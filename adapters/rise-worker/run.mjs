#!/usr/bin/env node
// Phase 2 adapter: run the real RISE Worker against recorded Jev answers, through a fault proxy,
// and write a shoke-history/v1 file. See docs/ADAPTER-RISE.md.
//
//   node adapters/rise-worker/run.mjs --rise PATH [--seed N] [--fault-rate PCT] [--mix CLASS]
//        [--schema 2|3] [--no-key] [--turns N] --out FILE
//
// No network: the only destination the proxy accepts is the provider URL, which it answers
// itself. Node built-ins only.

import { register } from 'node:module';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

register('./hooks.mjs', import.meta.url);

const FIXTURE = 'scripts/jev-eval-production-broad-baseline-2026-09-26.json';
const CASES = 'scripts/jev-eval-cases.json';
const PROVIDER_URL = 'https://openrouter.ai/api/alpha/decisions';
const SITE = 'https://rise.example';
const DEADLINE_MS = 8000;
const PACE_MENU = '100,150,200,250,300,400,500';
const PROBES = [
  { id: 'p1', intent: 'Help me drift off to sleep.', replay: 'rest-slow' },
  { id: 'p2', intent: 'Read slowly while I drift off, no music.', replay: 'quiet-combined' },
  { id: 'p3', intent: 'A neon city, but keep the screen dark, text only.', replay: 'spare' }
];
// Recorded field -> provider question. Middle and finale audio follow the recorded opening sound.
const RECORDED = {
  pace: 'pace', audio: 'audio', middleAudio: 'audio', finaleAudio: 'audio',
  visual: 'visualMode', visualStyle: 'visualStyle', chamberFace: 'chamberFace', fontSize: 'fontSize'
};
const MIXES = { all: ['http', 'timeout', 'truncate', 'menu'], http: ['http'], slow: ['timeout'],
  truncate: ['truncate'], menu: ['menu'] };

// SplitMix64, the same algorithm and reference vector as shoke-core's Rng.
class Rng {
  constructor(seed) { this.state = BigInt.asUintN(64, BigInt(seed)); }
  next() {
    this.state = BigInt.asUintN(64, this.state + 0x9E3779B97F4A7C15n);
    let z = this.state;
    z = BigInt.asUintN(64, (z ^ (z >> 30n)) * 0xBF58476D1CE4E5B9n);
    z = BigInt.asUintN(64, (z ^ (z >> 27n)) * 0x94D049BB133111EBn);
    return z ^ (z >> 31n);
  }
  below(n) { return n === 0 ? 0 : Number(this.next() % BigInt(n)); }
}
if (new Rng(0).next() !== 0xE220A8397B1DCDAFn) throw new Error('SplitMix64 does not match shoke-core');

function args(argv) {
  const o = { seed: 0, faultRate: 0, mix: 'all', schema: 3, key: true, turns: 2 };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    if (a === '--rise') o.rise = value();
    else if (a === '--out') o.out = value();
    else if (a === '--seed') o.seed = BigInt(value());
    else if (a === '--fault-rate') o.faultRate = Number(value());
    else if (a === '--mix') o.mix = value();
    else if (a === '--schema') o.schema = Number(value());
    else if (a === '--no-key') o.key = false;
    else if (a === '--turns') o.turns = Number(value());
    else throw new Error(`unknown argument ${a}`);
  }
  if (!o.rise || !o.out) throw new Error('--rise PATH and --out FILE are required');
  if (!(o.faultRate >= 0 && o.faultRate <= 100)) throw new Error('--fault-rate is a percentage');
  if (!MIXES[o.mix]) throw new Error(`--mix is one of ${Object.keys(MIXES).join(', ')}`);
  if (![2, 3].includes(o.schema)) throw new Error('--schema is 2 or 3');
  if (!(Number.isInteger(o.turns) && o.turns >= 1)) throw new Error('--turns is a whole number, 1 or more');
  return o;
}

// shoke-history/v1 value encoding: every byte except A-Za-z0-9 - . _ ~ : , / becomes %XX.
function enc(value) {
  let out = '';
  for (const b of new TextEncoder().encode(String(value))) {
    const c = String.fromCharCode(b);
    out += /[A-Za-z0-9\-._~:,/]/.test(c) ? c : `%${b.toString(16).toUpperCase().padStart(2, '0')}`;
  }
  return out;
}

function line(t, kind, fields) {
  return `${t} ${kind}${Object.entries(fields).map(([k, v]) => ` ${k}=${enc(v)}`).join('')}`;
}

const readJson = (rise, file) => JSON.parse(fs.readFileSync(path.join(rise, file), 'utf8'));

function catalog(rise) {
  const inventory = readJson(rise, 'src/content/archive/release-inventory.json');
  // The row shape RISE's own Worker tests use.
  const books = Object.keys(inventory).map(workId => ({
    work_id: workId, title: 'Fixture title', author: 'Fixture author',
    edition_id: inventory[workId].editionId, source_revision: inventory[workId].sourceRevision,
    fit_description: 'A thoughtful classic for reflective reading.',
    decision_criterion: 'Choose this for a reflective reading mood.', active: true
  }));
  // Production sound descriptions, from RISE's own seed file.
  const sql = fs.readFileSync(path.join(rise, 'scripts/seed-rise-sounds.sql'), 'utf8');
  const sounds = [...sql.matchAll(/\('([a-z-]+)',\s*'((?:[^']|'')*)',\s*(TRUE|FALSE)\)/g)]
    .filter(m => m[3] === 'TRUE')
    .map(m => ({ sound_id: m[1], decision_criterion: m[2].replaceAll("''", "'").replace(/\s+/g, ' '),
      active: true }));
  const options = [
    ...['literary', 'display', 'thick', 'jp', 'mono', 'sans', 'book']
      .map(id => ({ kind: 'chamberFace', id, description: 'Reviewed face.' })),
    ...['small', 'medium', 'large', 'xlarge', 'fit']
      .map(id => ({ kind: 'fontSize', id, description: 'Reviewed size.' }))
  ];
  return { books, sounds, options };
}

// One store for the whole run, so the decision cache and turn counters persist across requests.
function services(cat, current) {
  const store = new Map();
  const clone = value => JSON.parse(JSON.stringify(value));
  globalThis.__shoke = {
    neon: async query => query.includes('rise_jev_options') ? cat.options
      : query.includes('FROM rise_sounds') ? cat.sounds : cat.books,
    redis: {
      get: async key => {
        if (key.startsWith('rise:jev-decision:')) current.observed.key = key;
        return store.has(key) ? clone(store.get(key)) : null;
      },
      set: async (key, value) => { store.set(key, clone(value)); return 'OK'; },
      incr: async key => { const n = (store.get(key) || 0) + 1; store.set(key, n); return n; },
      expire: async () => 1
    }
  };
}

// The fault proxy: answers the provider call from the recording, or breaks it.
function proxy(plan, observed) {
  return async (url, init) => {
    if (String(url) !== PROVIDER_URL) throw new Error(`blocked a network call to ${url}`);
    observed.called = true;
    const body = JSON.parse(init.body);
    const offered = question => Object.keys(body.questions[question]?.criteria || {});
    const answers = {};
    const adapted = [];
    for (const question of Object.keys(body.questions)) {
      const choices = offered(question);
      let choice = question === 'book' ? choices[0] : plan.recorded[RECORDED[question]];
      if (choice === undefined || !choices.includes(choice)) {
        if (choice !== undefined) adapted.push(question);
        choice = choices[0];
      }
      answers[question] = { type: 'choice', choice };
    }
    observed.adapted = adapted;
    switch (plan.fault) {
      case 'http': return new Response('{}', { status: plan.code });
      case 'timeout': throw new DOMException('The operation was aborted due to timeout', 'TimeoutError');
      case 'truncate': return new Response('{"id":"fx","model":"typesafe/jev-1.13-20260917","ans',
        { headers: { 'Content-Type': 'application/json' } });
      case 'menu': answers.pace = { type: 'choice', choice: '999' }; break;
      default: break;
    }
    return Response.json({ id: `fx-${plan.id}`, model: 'typesafe/jev-1.13-20260917',
      provider: 'TypeSafe', answers });
  };
}

function soundRank(audio) {
  if (audio === 'silent') return 0;
  return audio === 'night-drive' ? 3 : 2;
}

async function main() {
  const o = args(process.argv.slice(2));
  const rise = path.resolve(o.rise);
  const { handleJevRecommend } = await import(pathToFileURL(path.join(rise, 'worker/jev-recommend.mjs')).href);
  const recorded = Object.fromEntries(readJson(rise, FIXTURE).rows.map(r => [r.id, r.decision]));
  const cases = readJson(rise, CASES).map(c => ({ id: c.id, intent: c.intent, replay: c.id }));
  const turn = [...cases, ...PROBES];
  const requests = Array.from({ length: o.turns }, () => turn).flat();
  const cat = catalog(rise);
  const bookIndex = new Map(cat.books.map((b, i) => [b.work_id, i]));
  const env = { DECISION_PROVIDER: 'jev', OPENROUTER_API_KEY: 'fixture-only',
    NEON_DATABASE_URL: 'postgresql://stand-in/rise', UPSTASH_REDIS_REST_URL: 'https://redis.stand-in',
    UPSTASH_REDIS_REST_TOKEN: 'stand-in' };
  let commit = 'unknown';
  try { commit = execFileSync('git', ['-C', rise, 'rev-parse', '--short', 'HEAD']).toString().trim(); } catch {}

  const out = ['shoke-history/v1'];
  const meta = { target: 'rise-worker', rise_commit: commit, fixture: FIXTURE, seed: o.seed,
    schema: o.schema, fault_rate: o.faultRate, mix: o.mix, deadline_ms: DEADLINE_MS, max_calls: 1,
    menu_wpm: PACE_MENU, floor: false, sound_rank: 'silent:0,night-drive:3,other:2' };
  // Written only when it differs from the default, so a default history is unchanged.
  if (o.turns !== 2) meta.turns = o.turns;
  for (const [k, v] of Object.entries(meta)) out.push(`meta ${k}=${enc(v)}`);

  const current = { observed: null };
  services(cat, current);
  const rng = new Rng(o.seed);
  const counts = { requests: 0, calls: 0, faults: 0, adapted: 0, plans: 0, errors: 0, cache: 0 };
  for (const [i, rq] of requests.entries()) {
    // Every draw happens for every request, so a cache hit never shifts later faults.
    const faulted = rng.below(10000) < o.faultRate * 100;
    const kinds = MIXES[o.mix];
    const fault = faulted ? kinds[rng.below(kinds.length)] : null;
    const code = [429, 500, 503][rng.below(3)];
    const latency = fault === 'timeout' ? DEADLINE_MS : fault === 'http' ? 30 + rng.below(70)
      : 600 + rng.below(1900);

    const id = `r${i + 1}`;
    const t = 1000 * i + 500;
    const observed = { called: false, key: null, adapted: [] };
    current.observed = observed;
    globalThis.fetch = proxy({ id: `${rq.replay}-${i + 1}`, recorded: recorded[rq.replay], fault, code },
      observed);
    const response = await handleJevRecommend(new Request(`${SITE}/api/jev-recommend`, {
      method: 'POST', headers: { Origin: SITE, 'Content-Type': 'application/json' },
      body: JSON.stringify({ intent: rq.intent, schemaVersion: o.schema })
    }), env);
    const body = await response.json();

    counts.requests++;
    out.push(line(t, 'req', { id, text: rq.intent, case: rq.id }));
    let end = t + 2;
    if (observed.called) {
      counts.calls++;
      if (fault) counts.faults++;
      if (observed.adapted.length) counts.adapted++;
      const call = { req: id, n: 1 };
      if (observed.adapted.length) call.adapted = observed.adapted.join(',');
      out.push(line(t + 1, 'call', call));
      end = t + 1 + latency;
      const status = fault === 'http' ? `http_${code}` : fault === 'timeout' ? 'timeout'
        : fault === 'truncate' ? 'truncated' : fault === 'menu' ? 'out_of_menu' : 'ok';
      out.push(line(end, 'resp', { req: id, n: 1, status }));
    }
    if (response.ok) {
      const c = body.config;
      const hit = body.decisionCacheStatus === 'hit';
      counts.plans++;
      if (hit) counts.cache++;
      const fields = { req: id, source: hit ? 'cache' : 'model' };
      if (o.key && observed.key) fields.key = observed.key;
      Object.assign(fields, { wpm: c.wpm, sound: soundRank(c.audio),
        visual: c.visualMode === 'off' ? 0 : c.visualPalette === 'neon' ? 3 : 1,
        book: bookIndex.get(body.workId) ?? 99, audio: c.audio, palette: c.visualPalette });
      out.push(line(end, 'decision', fields));
    } else {
      counts.errors++;
      // src/components/Portal.js shows the error and leaves the intent field as the reader typed it.
      out.push(line(end, 'error', { req: id, reason: body.error?.code || `http_${response.status}`,
        visible: true, preserved: true }));
    }
  }
  fs.writeFileSync(o.out, `${out.join('\n')}\n`);
  console.log(`rise-worker @${commit} seed=${o.seed} schema=${o.schema} fault-rate=${o.faultRate}% mix=${o.mix}`);
  console.log(Object.entries(counts).map(([k, v]) => `${k}=${v}`).join(' '));
  console.log(`history written to ${o.out}`);
}

main().catch(e => { console.error(`rise-worker: ${e.message}`); process.exit(2); });

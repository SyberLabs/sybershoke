# Phase 2: the RISE Worker adapter

The first target that is not a model. `adapters/rise-worker/` runs the **real** RISE Worker source
(`worker/jev-recommend.mjs`, `handleJevRecommend`) in Node, with a fault proxy in place of the
provider and in-memory stand-ins for Redis and Neon. It writes a `shoke-history/v1` file that
`shoke check` judges like any other.

## What is real, what is replayed, what is assumed

| Part | Source |
|---|---|
| Request handling, menu, validation, night-drive override, cache keys, error codes | **Real**: the RISE checkout given with `--rise` |
| Reader intents | **Real**: the 39 cases in RISE `scripts/jev-eval-cases.json`, plus 3 probe intents below |
| Provider answers | **Recorded**: RISE `scripts/jev-eval-production-broad-baseline-2026-09-26.json`, 39 live Jev responses captured from production on 2026-09-26. Test fixtures only, never training data. |
| Fields the recording lacks | The record keeps 6 fields per answer (pace, audio, visual mode, visual style, face, size). The other questions take the first choice the Worker offers. `book` takes the first eligible book. |
| Redis, Neon | Stand-ins: an in-memory map, and the catalog rows RISE's own tests use |
| Time | Virtual. Provider latency 600 to 2500 ms (**assumed**, as in the model); a call over 8000 ms is ended by the proxy as the Worker's `AbortSignal.timeout(8000)` would end it |
| What the reader sees on an error | **Read from code**: `src/components/Portal.js` shows the error and never clears the intent field, so an error is `visible=true preserved=true` |

A recorded audio choice may not be on the menu the Worker offers for that turn, because the Worker
shortlists 9 sounds per turn and rotates them. The proxy then substitutes the first offered sound
and marks the call `adapted=audio`. An answer the recording never gave is never presented as recorded.

### Mapping a Worker plan onto the history format

| History field | From the Worker's config |
|---|---|
| `wpm` | `wpm`. The menu is declared with `meta menu_wpm=100,150,200,250,300,400,500` |
| `sound` | A loudness rank read from RISE's own sound descriptions (see below), declared with `meta sound_rank` |
| `visual` | 0 when `visualMode` is `off`, 3 when the palette is `neon`, 1 otherwise |
| `book` | Position of `workId` in the 15-book release catalog |
| `key` | The decision cache key the Worker read or wrote, observed at the Redis stand-in |

### Sound rank

RISE does not rank its sounds by loudness. The adapter reads a rank from the words in RISE's own
sound descriptions (`scripts/seed-rise-sounds.sql`): 1 where the description says soft, gentle,
quiet or calm; 3 where it says beat, driving, fast or energy; 2 otherwise; `silent` is 0. This is
still an interpretation, not a ranking RISE owns.

| Rank | Sound | Word in RISE's description |
|---|---|---|
| 0 | silent | (no sound) |
| 1 | aurora | "Soft, spacious harmonics for calm ..." |
| 1 | piano | "Gentle piano melody ..." |
| 1 | lullaby | "A soft, rocking keyboard lullaby ... gentle care" |
| 1 | nocturne | "A quiet night piece ..." |
| 1 | starlight | "... for cosmic calm and night skies" |
| 3 | ragtime | "... buoyant energy" |
| 3 | night-drive | "Fast electronic beat ... high energy" |
| 2 | the other 17 | none of the words |

Near misses, left at 2: waltz says "three-beat", which is its meter; excited and thrilling say
"No beat"; only soft-rain's id says soft, not its description; bossa says "light", blues
"relaxed", chase "quick". The adapter checks that each cited word is still in RISE's description
and stops if it is not, so the table cannot drift from the source silently.

## Predictions, written before the first run

Scenario: the 39 cases, then three probes, in that order, one request per second; then the same 42
again (turn 2, to reach the cache). `--schema 3` is what the production client sends
(`src/components/Portal.js:411`).

Probes, each replaying a recorded answer:

- **p1** "Help me drift off to sleep." replays `rest-slow` (recorded pace 100).
- **p2** "Read slowly while I drift off, no music." replays `quiet-combined`.
- **p3** "A neon city, but keep the screen dark, text only." replays `spare` (control).

| # | Prediction | Confidence |
|---|---|---|
| P1 | Clean run, schema 3: I4 fails on p1 and p2 (wpm raised to 300 against "sleep" and "slowly"), on both turns. p3 passes: its no-visual words block the override. | High: read in code |
| P2 | Clean run, schema 2: p1 and p2 pass I4. Neon is not offered, so the override never fires. | High |
| P3 | Every injected provider fault ends in an error event and never a plan. With `--require-floor`, I7 fails on every faulted request. I2 accepts them (visible, preserved). | High |
| P4 | I5 never fails: one provider call per request (`meta max_calls=1`). | High |
| P5 | I1 never fails: the Worker validates every answer before admitting it. | High |
| P6 | I3 passes with the Worker's key. Deleting `key=` so I3 falls back to normalized text makes it fail wherever turn 2 lands in a different variation cohort and picks a different book. | Medium |
| P7 | Every timeout fails I2 by the time spent before the provider call: the Worker's 8 s covers the call, not the request. That is a question for the spec, not a Worker bug. | High |
| P8 | The clean 39 cases raise 4 to 10 I4 violations. Every one traces to this harness's oracle ("calm" and "gentle" read as slow, "soft" and "quiet" as rank ≤ 1) or to the sound-rank mapping, and none to the Worker. The RISE team's own labels would pass them. | Medium |
| P9 | At least one recorded audio choice is not offered on its turn and is marked `adapted=audio`. | Medium |
| P10 | The same seed gives a byte-identical history. | High |

Phase 2's exit test in the roadmap is "finds the keyword misfire and the missing fallback with a
replayable seed". P1 and P3 are those two findings; P10 is the replayable seed.

## Results against RISE `082b3fa`

Reproduce with `adapters/rise-worker/check.sh /path/to/RISE`, which asserts P1, P2, P3, P7, P8 and
P10.

| # | Outcome | Detail |
|---|---|---|
| P1 | **Confirmed** | "Help me drift off to sleep." comes back at 300 wpm with the night-drive beat and the neon palette; the recorded answer said 100 wpm. "Read slowly while I drift off, no music." comes back at 300 wpm and neon, and silent: the no-music words are honoured, the pace words are not. Both turns (r40, r41, r82, r83). The control p3 is untouched. |
| P2 | **Confirmed** | Schema 2: p1 at 100 wpm, p2 at 150. |
| P3 | **Confirmed** | Seed 7, 30% faults: all 25 faulted requests end in an error, none with a plan. I1, I3 and I5 pass. |
| P4, P5 | **Confirmed** | I5 and I1 pass in every run. |
| P6 | **Refuted** | I3 passes even without the Worker's key. With two asks per intent, a hit can only follow the model answer for its own cohort. Breaking it needs a third ask (cohort A answered, cohort B answered, then a hit on A), which this scenario never makes. Tested with `--turns` in round 2 below. |
| P7 | **Confirmed** | Seed 3, timeouts only: 14 of 14 end at 8001 ms, 1 ms past a per-request 8 s. Whether the 8 s is per request or per call is for the spec's owner. |
| P8 | **Confirmed in count, cause narrower** | Under the first sound rank (silent 0, night-drive 3, every other sound 2; commit `5253c37`), 7 I4 flags on the clean cases. All 7 were "soft" or "quiet" against a non-silent sound: the sound-rank mapping, not the Worker. None came from "calm" or "gentle"; those cases were recorded at 200 wpm or slower. With the rank read from RISE's descriptions, 4 remain: "I'd like a soft bossa nova backing." (r23, r65) and "Give the background a quiet mystery." (r26, r68). Jev chose the sound the reader named, and RISE describes bossa as "light" and mystery as "sparse", neither of which the rule reads as quiet. Whether they are quiet is for RISE to say. |
| P9 | **Confirmed** | 7 calls replayed a recorded sound that was not on that turn's 9-sound shortlist and were marked `adapted=audio`. One of them is p1; its misfire comes from the override, not the substitution (the recorded pace, 100, was kept). |
| P10 | **Confirmed** | Seed 7 twice gives byte-identical histories. |

**Not predicted.** A truncated provider answer reaches the reader as `DECISION_UNAVAILABLE`, "Jev
could not be reached." `response.json()` runs inside the `try` that wraps `fetch`, so a malformed
answer is reported as an unreachable one (9 of 9 truncations in seed 7). No invariant covers error
wording; this is a diagnosis problem, not a lost request.

**One departure from the design above.** Middle and finale audio replay the recorded opening sound
instead of the first offered choice. The first choice is `silent`, and it would have invented a
sound arc the recording never had.

The roadmap's phase 2 exit test ("finds the keyword misfire and the missing fallback with a
replayable seed") passes against the real Worker source, driven by recorded answers.

## Predictions, round 2

Written before any run with more than 2 turns. P6 was refuted because each intent was asked only
twice. `--turns N` asks the same 42 intents N times; the default stays 2.

What the Worker source says (`worker/jev-recommend.mjs`, `worker/jev-variance.mjs`):

- The variation cohort applies only to open-ended requests ("surprise me", "what should I read").
  None of the 42 intents is one, so the cohort is null and every decision key ends in `:0`.
- The key also covers the 9-sound shortlist. Where the intent's words do not fill the shortlist,
  the rest is filled in catalog order starting one place further on each turn (turn mod 24). The
  start only moves forward, so a key that changed does not come back until the start wraps to 0
  on turn 25.
- A recorded sound missing from a turn's shortlist is replaced with the first offered choice,
  which is `silent`. So two turns of one intent can give different plans.

| # | Prediction | Confidence |
|---|---|---|
| Q1 | 3 turns, with the Worker's key: I3 passes. | High |
| Q2 | 3 turns, `--no-key`: I3 still passes. A turn-3 hit can only reuse the answer from turn 2's key or an unbroken run of the same key, which is also the latest model answer for that text. The cohort case P6 described cannot occur with these intents. | Medium |
| Q3 | 25 turns, `--no-key`: I3 fails, only on turn-25 requests. Turn 25 reuses the turn-1 key. Where turn 24 wrote a different answer for the same text (the recorded sound was on one shortlist and replaced by `silent` on the other), the text-keyed check sees a stale hit. Between 1 and 10 flags. | Medium |
| Q4 | 25 turns, with the Worker's key: I3 passes. The Worker's hit equals the answer stored under its own key. | High |

### Results, round 2, against RISE `082b3fa`

`adapters/rise-worker/check.sh` asserts each of these. Seed 1, no faults.

| # | Outcome | Detail |
|---|---|---|
| Q1 | **Confirmed** | 3 turns with the key: I3 passes. |
| Q2 | **Confirmed** | 3 turns without the key: I3 passes. No turn-3 hit reuses a key other than the one behind the latest model answer for its text. |
| Q3 | **Refuted** | 25 turns without the key: I3 passes. Old keys do come back, and earlier than predicted: 70 hits reuse an older key than the latest model answer for the same text, the first on turn 23 (r925), because the shortlist start wraps inside the filled positions before it wraps in the catalog. But in all 70 the old plan equals the new one, down to the sound. The replay gives each intent one recorded answer, so two turns can only differ by a substituted sound, and none of these did. |
| Q4 | **Confirmed** | 25 turns with the key: I3 passes. |

So the Worker does serve an older key's entry for the same text, and I3 would catch it if the plan
differed (`i3_catches_a_cache_hit_that_differs_from_the_model`). What this adapter cannot do is make
the plans differ: that needs a provider that answers the same intent differently on different
turns, and one recorded answer per intent is not that. The cohort case P6 described never arises:
none of the 42 intents is open-ended, so every key ends in `:0`.

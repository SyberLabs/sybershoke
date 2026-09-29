use shoke_core::{check, Rng};
use shoke_jev::sim::{simulate, Config, Scenario};
use shoke_jev::text::{expectations, requests_night_drive, NightDrive};
use shoke_jev::workload::{self, Request};
use shoke_jev::{default_set, Profile};
use std::collections::BTreeMap;
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "texts" {
        let mut seen = BTreeMap::new();
        for seed in 0..3000u64 { for r in workload::generate(&mut Rng::new(seed), 60) { seen.entry(shoke_jev::text::normalize(&r.text)).or_insert(()); } }
        for k in seen.keys() { let rules: Vec<String> = expectations(k).iter().map(|r| format!("{:?}", r.expect)).collect(); println!("{k}\t{}\t{}", rules.join(","), requests_night_drive(k, NightDrive::Narrow)); }
        return;
    }
    if mode == "lying" {
        // CacheFallback, but the cache entry is labelled the way a real Worker would label it.
        let mut cfg = Config::new(Profile::Jev); cfg.floor = true; cfg.bugs.insert(shoke_jev::Bug::CacheFallback);
        let (mut honest, mut lying) = (0, 0);
        for seed in 0..200 {
            let sc = Scenario::generate(seed, 60, 2, shoke_jev::Mix::All);
            let mut h = simulate(&cfg, &sc);
            if !check(&default_set(false), &h).violations_of("I3").is_empty() { honest += 1; }
            for e in h.events.iter_mut() { for f in e.fields.iter_mut() { if f.0 == "origin" { f.1 = "model".into(); } } }
            if !check(&default_set(false), &h).violations_of("I3").is_empty() { lying += 1; }
        }
        println!("cache-fallback caught by I3: honest label {honest}/200, label says model {lying}/200");
        return;
    }
    if mode == "shrink" {
        use shoke_jev::{minimal, Bug, Mix};
        for bug in Bug::ALL { for mix in [Mix::All, Mix::Http, Mix::Slow, Mix::Cold, Mix::Truncate, Mix::OutOfMenu] {
            let mut cfg = Config::new(Profile::Jev); cfg.floor = matches!(bug, Bug::CacheFallback); cfg.bugs.insert(bug);
            let invs = default_set(false);
            for seed in 0..12 {
                let sc = Scenario::generate(seed, 60, 2, mix);
                let before = check(&invs, &simulate(&cfg, &sc));
                if before.violations_of(bug.target()).is_empty() { continue; }
                let out = minimal(&cfg, &sc, &invs, bug.target(), 500);
                let after = check(&invs, &simulate(&cfg, &sc.with_faults(out.faults.clone())));
                let subj = |r: &shoke_core::Report| r.violations_of(bug.target()).iter().map(|v| v.subject.clone()).collect::<Vec<_>>();
                let f: Vec<String> = out.faults.iter().map(|f| f.to_string()).collect();
                println!("{} {} seed={} runs={} {:?} before={:?} after={:?}", bug.name(), mix.name(), seed, out.runs, f, subj(&before), subj(&after));
            }
        }}
        return;
    }
    if mode == "tight" {
        // For each stalled shrink result: does a 250 ms window at one failing call still fail?
        use shoke_jev::{minimal, Bug, Mix, JevFault};
        let cases = [(Bug::RetryStorm, Mix::Http, 4u64), (Bug::RetryStorm, Mix::All, 5), (Bug::NoValidation, Mix::All, 5), (Bug::NoDeadline, Mix::All, 0), (Bug::LoseRequestOnError, Mix::Http, 5)];
        for (bug, mix, seed) in cases {
            let mut cfg = Config::new(Profile::Jev); cfg.bugs.insert(bug);
            let invs = default_set(false);
            let sc = Scenario::generate(seed, 60, 2, mix);
            let out = minimal(&cfg, &sc, &invs, bug.target(), 500);
            let f = out.faults[0].clone();
            let h = simulate(&cfg, &sc.with_faults(out.faults.clone()));
            let mut best: Option<JevFault> = None;
            for c in h.of_kind("call").filter(|c| f.active(c.t)) {
                let t = JevFault { at: c.t, dur: 250, kind: f.kind };
                if check(&invs, &simulate(&cfg, &sc.with_faults(vec![t.clone()]))).failing().contains(&bug.target()) { best = Some(t); break; }
            }
            println!("{} {} seed {}: shrinker says {} ; tighter that still fails: {}", bug.name(), mix.name(), seed, f, best.map(|b| b.to_string()).unwrap_or("none found".into()));
        }
        return;
    }
    // Adversarial phrases: one request each, through the CORRECT pipeline (floor on), check I4.
    let phrases = std::fs::read_to_string(std::env::args().nth(2).unwrap()).unwrap();
    for p in phrases.lines().filter(|l| !l.is_empty()) {
        let rules: Vec<String> = expectations(p).iter().map(|r| format!("{:?}", r.expect)).collect();
        let sc = Scenario { seed: 1, requests: vec![Request { id: "r1".into(), at: 500, text: p.into() }], faults: vec![] };
        let mut cfg = Config::new(Profile::Jev); cfg.floor = true;
        let rep = check(&default_set(true), &simulate(&cfg, &sc));
        let v: Vec<String> = rep.violations_of("I4").iter().map(|v| v.detail.clone()).collect();
        println!("{p}\t{}\t{}\t{}", rules.join(","), requests_night_drive(p, NightDrive::Narrow), if v.is_empty() { "PASS".into() } else { format!("FAIL {}", v.join(" | ")) });
    }
}

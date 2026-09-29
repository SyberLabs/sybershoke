//! The request path, in virtual milliseconds: cache, provider call, retry, deadline, keyword
//! override, and (optionally) a preset floor. Every branch writes events; nothing here decides
//! whether the run was correct. That is the checker's job.
//!
//! Each seeded [`Bug`] removes or breaks one safeguard, so the harness can be shown to catch it.

use crate::faults::{self, JevFault, Mix};
use crate::menu::{Plan, NEON, SYNTH};
use crate::provider::{self, Kind, Profile};
use crate::text::{normalize, requests_night_drive, NightDrive};
use crate::workload::{self, Request};
use shoke_core::{mix, Event, History, Rng};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bug {
    /// The night-drive override matches bare words such as `drift` and `neon`.
    BareKeywordOverride,
    /// Answers are not checked against the menu before being admitted.
    NoValidation,
    /// Failed calls are retried immediately, with no cap.
    RetryStorm,
    /// There is no client-side deadline; the Worker waits for the provider however long it takes.
    NoDeadline,
    /// A fallback plan is stored in the decision cache as if the model had produced it.
    CacheFallback,
    /// When a request fails, the request text is dropped along with the error.
    LoseRequestOnError,
}

impl Bug {
    pub const ALL: [Bug; 6] = [
        Bug::BareKeywordOverride,
        Bug::NoValidation,
        Bug::RetryStorm,
        Bug::NoDeadline,
        Bug::CacheFallback,
        Bug::LoseRequestOnError,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Bug::BareKeywordOverride => "bare-keyword-override",
            Bug::NoValidation => "no-validation",
            Bug::RetryStorm => "retry-storm",
            Bug::NoDeadline => "no-deadline",
            Bug::CacheFallback => "cache-fallback",
            Bug::LoseRequestOnError => "lose-request-on-error",
        }
    }

    pub fn parse(s: &str) -> Option<Bug> {
        Bug::ALL.iter().copied().find(|b| b.name() == s)
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Bug::BareKeywordOverride => {
                "night-drive override fires on bare words, so \"drift off to sleep\" gets 300 wpm"
            }
            Bug::NoValidation => "an out-of-menu answer is admitted as the plan",
            Bug::RetryStorm => "a failing provider is retried without a cap or a pause",
            Bug::NoDeadline => {
                "the Worker waits for a slow provider instead of giving up at the deadline"
            }
            Bug::CacheFallback => {
                "a fallback plan is cached, so it outlives the outage that caused it"
            }
            Bug::LoseRequestOnError => "a failed request loses the reader's text",
        }
    }

    /// The invariant that should catch this bug.
    pub fn target(&self) -> &'static str {
        match self {
            Bug::BareKeywordOverride => "I4",
            Bug::NoValidation => "I1",
            Bug::RetryStorm => "I5",
            Bug::NoDeadline => "I2",
            Bug::CacheFallback => "I3",
            Bug::LoseRequestOnError => "I2",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub profile: Profile,
    /// Client-side deadline, measured from the request's arrival.
    pub deadline_ms: u64,
    /// Provider calls allowed per request, first try included.
    pub max_calls: u32,
    /// Whether a preset floor exists to answer when the provider fails.
    pub floor: bool,
    pub bugs: BTreeSet<Bug>,
}

impl Config {
    pub fn new(profile: Profile) -> Config {
        Config {
            profile,
            deadline_ms: 8000,
            max_calls: 2,
            floor: false,
            bugs: BTreeSet::new(),
        }
    }

    pub fn has(&self, bug: Bug) -> bool {
        self.bugs.contains(&bug)
    }
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub seed: u64,
    pub requests: Vec<Request>,
    pub faults: Vec<JevFault>,
}

/// Fault randomness has its own stream so that changing the fault count never changes the
/// traffic, which keeps a seed meaningful while a scenario is being shrunk.
const FAULT_STREAM: u64 = 0xFA17_5EED;

impl Scenario {
    pub fn generate(seed: u64, n_requests: usize, n_faults: usize, fault_mix: Mix) -> Scenario {
        let requests = workload::generate(&mut Rng::new(seed), n_requests);
        let horizon = requests.last().map_or(0, |r| r.at) + 10_000;
        let faults = faults::generate(
            &mut Rng::new(seed ^ FAULT_STREAM),
            n_faults,
            horizon,
            fault_mix,
        );
        Scenario {
            seed,
            requests,
            faults,
        }
    }

    pub fn with_faults(&self, faults: Vec<JevFault>) -> Scenario {
        Scenario {
            seed: self.seed,
            requests: self.requests.clone(),
            faults,
        }
    }
}

enum Eff {
    Plan(Plan),
    Fail(&'static str),
}

fn http_label(code: u16) -> &'static str {
    match code {
        429 => "http_429",
        503 => "http_503",
        _ => "http_500",
    }
}

fn decision(t: u64, id: &str, source: &str, origin: Option<&str>, p: &Plan) -> Event {
    let mut e = Event::new(t, "decision")
        .with("req", id)
        .with("source", source);
    if let Some(o) = origin {
        e = e.with("origin", o);
    }
    e.with("wpm", p.wpm)
        .with("sound", p.sound)
        .with("visual", p.visual)
        .with("book", p.book)
}

pub fn simulate(cfg: &Config, sc: &Scenario) -> History {
    let mut h = History::new();
    h.add_meta("target", "jev-pipeline-model");
    h.add_meta("seed", sc.seed);
    h.add_meta("profile", cfg.profile.name());
    h.add_meta("requests", sc.requests.len());
    h.add_meta("deadline_ms", cfg.deadline_ms);
    h.add_meta("max_calls", cfg.max_calls);
    h.add_meta("floor", cfg.floor);
    let bugs: Vec<&str> = cfg.bugs.iter().map(|b| b.name()).collect();
    h.add_meta(
        "bugs",
        if bugs.is_empty() {
            "none".to_string()
        } else {
            bugs.join(",")
        },
    );
    for f in &sc.faults {
        h.add_meta("fault", f);
    }

    let mode = if cfg.has(Bug::BareKeywordOverride) {
        NightDrive::Bare
    } else {
        NightDrive::Narrow
    };
    let storm = cfg.has(Bug::RetryStorm);
    let no_deadline = cfg.has(Bug::NoDeadline);

    // key -> (ready_at, plan, origin)
    let mut cache: HashMap<String, (u64, Plan, &'static str)> = HashMap::new();
    // The host starts warm at t=0. Starting cold would make request 1 of every run hit the
    // 35 s cold start and drown out the effect worth measuring: idle gaps in the traffic.
    let mut last_call: Option<u64> = Some(0);

    for (idx, rq) in sc.requests.iter().enumerate() {
        h.push(
            Event::new(rq.at, "req")
                .with("id", &rq.id)
                .with("text", &rq.text),
        );
        let key = normalize(&rq.text);

        if let Some(&(ready_at, plan, origin)) = cache.get(&key) {
            if ready_at <= rq.at {
                h.push(decision(rq.at + 2, &rq.id, "cache", Some(origin), &plan));
                continue;
            }
        }

        let deadline_at = rq.at + cfg.deadline_ms;
        let mut t = rq.at + 3;
        let mut calls = 0u32;
        let outcome: Result<(u64, Plan), (u64, &'static str)> = loop {
            calls += 1;
            let mut rng = Rng::new(mix(sc.seed, idx as u64, calls as u64));
            h.push(Event::new(t, "call").with("req", &rq.id).with("n", calls));
            let resp = provider::call(
                cfg.profile,
                &sc.faults,
                &rq.text,
                t,
                &mut last_call,
                &mut rng,
            );
            let arrive = t + resp.latency;

            let (rt, eff) = if !no_deadline && arrive > deadline_at {
                (deadline_at, Eff::Fail("timeout"))
            } else {
                let eff = match resp.kind {
                    Kind::Ok(p) => Eff::Plan(p),
                    Kind::Http(code) => Eff::Fail(http_label(code)),
                    Kind::Truncated => Eff::Fail("truncated"),
                    Kind::OutOfMenu(p) => {
                        if cfg.has(Bug::NoValidation) {
                            Eff::Plan(p)
                        } else {
                            Eff::Fail("out_of_menu")
                        }
                    }
                };
                (arrive, eff)
            };
            let status = match &eff {
                Eff::Plan(_) => "ok",
                Eff::Fail(reason) => reason,
            };
            h.push(
                Event::new(rt, "resp")
                    .with("req", &rq.id)
                    .with("n", calls)
                    .with("status", status),
            );

            match eff {
                Eff::Plan(p) => break Ok((rt, p)),
                Eff::Fail(reason) => {
                    let under_cap = if storm {
                        calls < 40
                    } else {
                        calls < cfg.max_calls
                    };
                    let next = rt + if storm { 0 } else { 250 };
                    let has_time = no_deadline || next < deadline_at;
                    if reason != "timeout" && under_cap && has_time {
                        t = next;
                        continue;
                    }
                    break Err((rt, reason));
                }
            }
        };

        match outcome {
            Ok((rt, mut plan)) => {
                if requests_night_drive(&rq.text, mode) {
                    plan.wpm = 300;
                    plan.sound = SYNTH;
                    plan.visual = NEON;
                }
                h.push(decision(rt, &rq.id, "model", None, &plan));
                cache.insert(key, (rt, plan, "model"));
            }
            Err((rt, reason)) => {
                if cfg.floor {
                    let plan = provider::preset_floor(&rq.text);
                    h.push(decision(rt, &rq.id, "fallback", None, &plan));
                    if cfg.has(Bug::CacheFallback) {
                        cache.insert(key, (rt, plan, "fallback"));
                    }
                } else {
                    h.push(
                        Event::new(rt, "error")
                            .with("req", &rq.id)
                            .with("reason", reason)
                            .with("visible", true)
                            .with("preserved", !cfg.has(Bug::LoseRequestOnError)),
                    );
                }
            }
        }
    }

    h.sort();
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Profile;

    fn scenario(seed: u64) -> Scenario {
        Scenario::generate(seed, 40, 2, Mix::All)
    }

    #[test]
    fn same_seed_gives_identical_history_text() {
        let cfg = Config::new(Profile::Jev);
        let a = simulate(&cfg, &scenario(11)).to_text();
        let b = simulate(&cfg, &scenario(11)).to_text();
        assert_eq!(a, b);
    }

    #[test]
    fn every_request_gets_exactly_one_terminal_event_in_a_clean_run() {
        let cfg = Config::new(Profile::Jev);
        let sc = Scenario::generate(3, 60, 0, Mix::All);
        let h = simulate(&cfg, &sc);
        assert_eq!(h.of_kind("req").count(), 60);
        let terminals = h.of_kind("decision").count() + h.of_kind("error").count();
        assert_eq!(terminals, 60);
    }

    #[test]
    fn scale_to_zero_is_warm_at_start_and_cold_only_after_an_idle_gap() {
        let cfg = Config::new(Profile::KevScaleToZero);
        let sc = Scenario::generate(3, 80, 0, Mix::All);
        let h = simulate(&cfg, &sc);
        let first = h
            .of_kind("req")
            .next()
            .unwrap()
            .get("id")
            .unwrap()
            .to_string();
        let first_end = h
            .events
            .iter()
            .find(|e| e.kind == "decision" && e.get("req") == Some(first.as_str()))
            .expect("first request is answered");
        assert!(first_end.t < 5000, "first request must not be cold");
        let has_gap = sc.requests.windows(2).any(|w| w[1].at - w[0].at > 120_000);
        let errors = h.of_kind("error").count();
        // A cache hit after a gap never calls the provider, so a gap does not force an error;
        // but with no gap there must be none.
        assert!(errors == 0 || has_gap, "errors need an idle gap");
        assert!(h
            .of_kind("error")
            .all(|e| e.get("reason") == Some("timeout")));
    }

    #[test]
    fn history_is_time_ordered() {
        let cfg = Config::new(Profile::Jev);
        let h = simulate(&cfg, &scenario(5));
        assert!(h.events.windows(2).all(|w| w[0].t <= w[1].t));
    }

    #[test]
    fn traffic_does_not_depend_on_the_fault_count() {
        let a = Scenario::generate(8, 30, 0, Mix::All);
        let b = Scenario::generate(8, 30, 5, Mix::All);
        assert_eq!(a.requests, b.requests);
    }

    #[test]
    fn meta_records_the_configuration() {
        let mut cfg = Config::new(Profile::KevScaleToZero);
        cfg.floor = true;
        cfg.bugs.insert(Bug::RetryStorm);
        let h = simulate(&cfg, &scenario(1));
        assert_eq!(h.meta("profile"), Some("kev-cold"));
        assert_eq!(h.meta("floor"), Some("true"));
        assert_eq!(h.meta("bugs"), Some("retry-storm"));
        assert_eq!(h.meta_u64("deadline_ms"), Some(8000));
    }

    #[test]
    fn bug_names_round_trip() {
        for b in Bug::ALL {
            assert_eq!(Bug::parse(b.name()), Some(b));
        }
        assert_eq!(Bug::parse("nope"), None);
    }
}

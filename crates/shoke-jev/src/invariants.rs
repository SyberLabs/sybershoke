//! Invariants over a history of the Jev/Kev request path.
//!
//! | Id | Says |
//! |----|------|
//! | I1 | Every plan admitted to the reader is on the menu. |
//! | I2 | Every request ends exactly once, within the deadline, and a failure keeps the reader's text. |
//! | I3 | The decision cache never serves a degraded or a different plan. |
//! | I4 | Explicit words in a request are honoured: slow is not fast, silent is not loud. |
//! | I5 | No request makes more provider calls than the cap. |
//! | I7 | (policy, optional) Every request is answered with a plan, even when the provider fails. |
//!
//! I6 in the roadmap is the Fanout invariant (nothing lost, nothing accepted twice). It belongs to
//! the Fanout target, not to this one.
//!
//! All of them read only the history, and take deadline and cap from its `meta` lines, so they
//! work on a file written by anything.

use crate::menu::Plan;
use crate::text::{expectations, normalize};
use shoke_core::{Event, History, Invariant, Violation};
use std::collections::HashMap;

const DEFAULT_DEADLINE_MS: u64 = 8000;
const DEFAULT_MAX_CALLS: u64 = 2;

pub fn default_set(require_floor: bool) -> Vec<Box<dyn Invariant>> {
    let mut v: Vec<Box<dyn Invariant>> = vec![
        Box::new(PlansOnMenu),
        Box::new(AnswerOrVisibleError),
        Box::new(CacheNeverDegraded),
        Box::new(ExplicitWordsHonoured),
        Box::new(RetryCap),
    ];
    if require_floor {
        v.push(Box::new(Availability));
    }
    v
}

fn plan_of(e: &Event) -> Option<Plan> {
    Some(Plan {
        wpm: e.get("wpm")?.parse().ok()?,
        sound: e.get("sound")?.parse().ok()?,
        visual: e.get("visual")?.parse().ok()?,
        book: e.get("book")?.parse().ok()?,
    })
}

fn req_of(e: &Event) -> &str {
    e.get("req").unwrap_or("?")
}

fn texts(h: &History) -> HashMap<String, String> {
    h.of_kind("req")
        .filter_map(|e| Some((e.get("id")?.to_string(), e.get("text")?.to_string())))
        .collect()
}

pub struct PlansOnMenu;

impl Invariant for PlansOnMenu {
    fn id(&self) -> &'static str {
        "I1"
    }
    fn name(&self) -> &'static str {
        "admitted-plans-valid"
    }
    fn check(&self, h: &History) -> Vec<Violation> {
        let mut out = Vec::new();
        for e in h.of_kind("decision") {
            match plan_of(e) {
                None => out.push(Violation {
                    invariant: "I1",
                    subject: req_of(e).to_string(),
                    at: e.t,
                    detail: "decision has missing or non-numeric fields".into(),
                }),
                Some(p) => {
                    for problem in p.problems() {
                        out.push(Violation {
                            invariant: "I1",
                            subject: req_of(e).to_string(),
                            at: e.t,
                            detail: problem,
                        });
                    }
                }
            }
        }
        out
    }
}

pub struct AnswerOrVisibleError;

impl Invariant for AnswerOrVisibleError {
    fn id(&self) -> &'static str {
        "I2"
    }
    fn name(&self) -> &'static str {
        "answer-or-visible-error"
    }
    fn check(&self, h: &History) -> Vec<Violation> {
        let deadline = h.meta_u64("deadline_ms").unwrap_or(DEFAULT_DEADLINE_MS);
        let mut terminals: HashMap<&str, Vec<&Event>> = HashMap::new();
        for e in &h.events {
            if e.kind == "decision" || e.kind == "error" {
                terminals.entry(req_of(e)).or_default().push(e);
            }
        }
        let mut out = Vec::new();
        for rq in h.of_kind("req") {
            let id = rq.get("id").unwrap_or("?");
            let ends = terminals.get(id).map(|v| v.as_slice()).unwrap_or(&[]);
            if ends.is_empty() {
                out.push(Violation {
                    invariant: "I2",
                    subject: id.to_string(),
                    at: rq.t,
                    detail: "request never got a plan or an error".into(),
                });
                continue;
            }
            if ends.len() > 1 {
                out.push(Violation {
                    invariant: "I2",
                    subject: id.to_string(),
                    at: ends[1].t,
                    detail: format!("request ended {} times", ends.len()),
                });
            }
            let end = ends[0];
            let took = end.t.saturating_sub(rq.t);
            if took > deadline {
                out.push(Violation {
                    invariant: "I2",
                    subject: id.to_string(),
                    at: end.t,
                    detail: format!("answered after {took} ms, deadline is {deadline} ms"),
                });
            }
            if end.kind == "error" {
                if end.get("visible") != Some("true") {
                    out.push(Violation {
                        invariant: "I2",
                        subject: id.to_string(),
                        at: end.t,
                        detail: "error was not shown to the reader".into(),
                    });
                }
                if end.get("preserved") != Some("true") {
                    out.push(Violation {
                        invariant: "I2",
                        subject: id.to_string(),
                        at: end.t,
                        detail: "error dropped the reader's request text".into(),
                    });
                }
            }
        }
        out
    }
}

pub struct CacheNeverDegraded;

impl Invariant for CacheNeverDegraded {
    fn id(&self) -> &'static str {
        "I3"
    }
    fn name(&self) -> &'static str {
        "cache-never-degraded"
    }
    fn check(&self, h: &History) -> Vec<Violation> {
        let texts = texts(h);
        let mut model_plan: HashMap<String, Plan> = HashMap::new();
        let mut out = Vec::new();
        for e in h.of_kind("decision") {
            let id = req_of(e);
            let Some(key) = texts.get(id).map(|t| normalize(t)) else {
                continue;
            };
            let Some(plan) = plan_of(e) else { continue };
            match e.get("source") {
                Some("model") => {
                    model_plan.insert(key, plan);
                }
                Some("cache") => {
                    if e.get("origin") != Some("model") {
                        out.push(Violation {
                            invariant: "I3",
                            subject: id.to_string(),
                            at: e.t,
                            detail: format!(
                                "cache served a {} plan as if the model had produced it",
                                e.get("origin").unwrap_or("unknown-origin")
                            ),
                        });
                    } else if let Some(prev) = model_plan.get(&key) {
                        if *prev != plan {
                            out.push(Violation {
                                invariant: "I3",
                                subject: id.to_string(),
                                at: e.t,
                                detail: "cache served a plan that differs from the model's".into(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

pub struct ExplicitWordsHonoured;

impl Invariant for ExplicitWordsHonoured {
    fn id(&self) -> &'static str {
        "I4"
    }
    fn name(&self) -> &'static str {
        "explicit-words-honoured"
    }
    fn check(&self, h: &History) -> Vec<Violation> {
        let texts = texts(h);
        let mut out = Vec::new();
        for e in h.of_kind("decision") {
            let id = req_of(e);
            let (Some(text), Some(plan)) = (texts.get(id), plan_of(e)) else {
                continue;
            };
            for rule in expectations(text) {
                if !rule.satisfied_by(&plan) {
                    out.push(Violation {
                        invariant: "I4",
                        subject: id.to_string(),
                        at: e.t,
                        detail: format!(
                            "{} (request {:?}, source {})",
                            rule.explain(&plan),
                            text,
                            e.get("source").unwrap_or("?")
                        ),
                    });
                }
            }
        }
        out
    }
}

pub struct RetryCap;

impl Invariant for RetryCap {
    fn id(&self) -> &'static str {
        "I5"
    }
    fn name(&self) -> &'static str {
        "retry-cap"
    }
    fn check(&self, h: &History) -> Vec<Violation> {
        let cap = h.meta_u64("max_calls").unwrap_or(DEFAULT_MAX_CALLS);
        let mut calls: HashMap<&str, (u64, u64)> = HashMap::new();
        for e in h.of_kind("call") {
            let entry = calls.entry(req_of(e)).or_insert((0, e.t));
            entry.0 += 1;
            entry.1 = e.t;
        }
        let mut over: Vec<_> = calls.into_iter().filter(|(_, (n, _))| *n > cap).collect();
        over.sort_by_key(|(_, (_, last))| *last);
        over.into_iter()
            .map(|(id, (n, last))| Violation {
                invariant: "I5",
                subject: id.to_string(),
                at: last,
                detail: format!("{n} provider calls, cap is {cap}"),
            })
            .collect()
    }
}

/// Policy invariant, off by default: with a preset floor in place, no request may end in an error.
pub struct Availability;

impl Invariant for Availability {
    fn id(&self) -> &'static str {
        "I7"
    }
    fn name(&self) -> &'static str {
        "always-a-plan"
    }
    fn check(&self, h: &History) -> Vec<Violation> {
        h.of_kind("error")
            .map(|e| Violation {
                invariant: "I7",
                subject: req_of(e).to_string(),
                at: e.t,
                detail: format!(
                    "no plan delivered ({})",
                    e.get("reason").unwrap_or("unknown reason")
                ),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shoke_core::check;

    fn req(t: u64, id: &str, text: &str) -> Event {
        Event::new(t, "req").with("id", id).with("text", text)
    }

    fn dec(t: u64, id: &str, source: &str, wpm: u32, sound: u8, visual: u8) -> Event {
        Event::new(t, "decision")
            .with("req", id)
            .with("source", source)
            .with("wpm", wpm)
            .with("sound", sound)
            .with("visual", visual)
            .with("book", 0)
    }

    fn history(events: Vec<Event>) -> History {
        let mut h = History::new();
        h.add_meta("deadline_ms", 8000);
        h.add_meta("max_calls", 2);
        for e in events {
            h.push(e);
        }
        h
    }

    fn failing(h: &History, floor: bool) -> Vec<&'static str> {
        check(&default_set(floor), h).failing()
    }

    #[test]
    fn a_clean_history_passes() {
        let h = history(vec![
            req(0, "r1", "slow and quiet"),
            dec(900, "r1", "model", 150, 1, 1),
        ]);
        assert!(failing(&h, true).is_empty());
    }

    #[test]
    fn i1_catches_an_out_of_menu_plan() {
        let h = history(vec![req(0, "r1", "x"), dec(500, "r1", "model", 999, 2, 1)]);
        assert_eq!(failing(&h, false), vec!["I1"]);
    }

    #[test]
    fn i2_catches_late_missing_and_lossy_answers() {
        let late = history(vec![
            req(0, "r1", "x"),
            dec(12_000, "r1", "model", 200, 2, 1),
        ]);
        assert_eq!(failing(&late, false), vec!["I2"]);
        let missing = history(vec![req(0, "r1", "x")]);
        assert_eq!(failing(&missing, false), vec!["I2"]);
        let lossy = history(vec![
            req(0, "r1", "x"),
            Event::new(100, "error")
                .with("req", "r1")
                .with("visible", true)
                .with("preserved", false),
        ]);
        assert_eq!(failing(&lossy, false), vec!["I2"]);
    }

    #[test]
    fn i2_accepts_a_visible_preserved_error() {
        let h = history(vec![
            req(0, "r1", "x"),
            Event::new(8000, "error")
                .with("req", "r1")
                .with("visible", true)
                .with("preserved", true),
        ]);
        assert!(failing(&h, false).is_empty());
    }

    #[test]
    fn i3_catches_a_cached_fallback() {
        let mut cached = dec(9000, "r2", "cache", 200, 2, 1);
        cached = cached.with("origin", "fallback");
        let h = history(vec![
            req(0, "r1", "Hello"),
            dec(100, "r1", "fallback", 200, 2, 1),
            req(9000, "r2", "hello"),
            cached,
        ]);
        assert_eq!(failing(&h, false), vec!["I3"]);
    }

    #[test]
    fn i3_catches_a_cache_hit_that_differs_from_the_model() {
        let h = history(vec![
            req(0, "r1", "hello"),
            dec(100, "r1", "model", 200, 2, 1),
            req(9000, "r2", "HELLO"),
            dec(9002, "r2", "cache", 300, 2, 1).with("origin", "model"),
        ]);
        assert_eq!(failing(&h, false), vec!["I3"]);
    }

    #[test]
    fn i4_catches_a_fast_plan_for_sleep() {
        let h = history(vec![
            req(0, "r1", "drift off to sleep"),
            dec(500, "r1", "model", 300, 3, 3),
        ]);
        assert_eq!(failing(&h, false), vec!["I4"]);
    }

    #[test]
    fn i5_catches_a_retry_storm() {
        let mut events = vec![req(0, "r1", "x")];
        for n in 1..=5 {
            events.push(Event::new(n * 10, "call").with("req", "r1").with("n", n));
        }
        events.push(dec(100, "r1", "model", 200, 2, 1));
        assert_eq!(failing(&history(events), false), vec!["I5"]);
    }

    #[test]
    fn i7_is_policy_and_off_by_default() {
        let h = history(vec![
            req(0, "r1", "x"),
            Event::new(8000, "error")
                .with("req", "r1")
                .with("reason", "timeout")
                .with("visible", true)
                .with("preserved", true),
        ]);
        assert!(failing(&h, false).is_empty());
        assert_eq!(failing(&h, true), vec!["I7"]);
    }

    #[test]
    fn deadline_and_cap_come_from_the_file() {
        let mut h = History::new();
        h.add_meta("deadline_ms", 100);
        h.push(req(0, "r1", "x"));
        h.push(dec(500, "r1", "model", 200, 2, 1));
        assert_eq!(failing(&h, false), vec!["I2"]);
    }
}

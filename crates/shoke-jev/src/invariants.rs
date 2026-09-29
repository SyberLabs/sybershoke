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
//! work on a file written by anything. [`shoke_core::check`] hands them events in time order.

use crate::menu::Plan;
use crate::text::{expectations, normalize};
use shoke_core::{Event, History, Invariant, Violation};
use std::collections::HashMap;

const DEFAULT_DEADLINE_MS: u64 = 8000;
const DEFAULT_MAX_CALLS: u64 = 2;

/// The deadline and call cap a history declares, or the defaults when it declares none. A value
/// that does not parse, or a key given twice with different values, is an error: the file sets its
/// own bar, so an unreadable bar must never quietly become the default.
pub fn limits(h: &History) -> Result<(u64, u64), String> {
    let one = |key: &str, default: u64| -> Result<u64, String> {
        let mut values = h.meta.iter().filter(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let Some(first) = values.next() else {
            return Ok(default);
        };
        if let Some(other) = values.find(|v| *v != first) {
            return Err(format!("meta {key} is given twice: `{first}` and `{other}`"));
        }
        first
            .parse()
            .map_err(|_| format!("meta {key}=`{first}` is not a whole number of the right unit"))
    };
    Ok((
        one("deadline_ms", DEFAULT_DEADLINE_MS)?,
        one("max_calls", DEFAULT_MAX_CALLS)?,
    ))
}

fn bad_meta(invariant: &'static str, detail: String) -> Vec<Violation> {
    vec![Violation {
        invariant,
        subject: "meta".into(),
        at: 0,
        detail,
    }]
}

/// The cache key of a decision: the system's own key when it records one (the RISE Worker keys on
/// the exact intent plus a variation cohort), otherwise the normalized request text.
fn cache_key(e: &Event, text: Option<&str>) -> Option<String> {
    match e.get("key") {
        Some(k) => Some(k.to_string()),
        None => text.map(normalize),
    }
}

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

/// Each decision paired with the text of the latest request of its id that arrived before it.
/// Events must be in time order, which [`shoke_core::check`] guarantees.
fn decisions_with_text(h: &History) -> Vec<(&Event, Option<&str>)> {
    let mut text: HashMap<&str, &str> = HashMap::new();
    let mut out = Vec::new();
    for e in &h.events {
        match e.kind.as_str() {
            "req" => {
                if let (Some(id), Some(t)) = (e.get("id"), e.get("text")) {
                    text.insert(id, t);
                }
            }
            "decision" => out.push((e, text.get(req_of(e)).copied())),
            _ => {}
        }
    }
    out
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
        // A real system declares its own pace menu (`meta menu_wpm=100,150,...`).
        let wpm_menu: Vec<u32> = match h.meta("menu_wpm") {
            None => crate::menu::WPM.to_vec(),
            Some(list) => match list.split(',').map(str::parse).collect() {
                Ok(v) => v,
                Err(_) => return bad_meta("I1", format!("meta menu_wpm=`{list}` is not a list")),
            },
        };
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
                    for problem in p.problems_with(&wpm_menu) {
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
        let deadline = match limits(h) {
            Ok((deadline, _)) => deadline,
            Err(e) => return bad_meta("I2", e),
        };
        let mut out = Vec::new();
        let mut arrivals: HashMap<&str, u64> = HashMap::new();
        for rq in h.of_kind("req") {
            let id = rq.get("id").unwrap_or("?");
            if arrivals.insert(id, rq.t).is_some() {
                out.push(Violation {
                    invariant: "I2",
                    subject: id.to_string(),
                    at: rq.t,
                    detail: "request id is used by more than one request".into(),
                });
            }
        }
        let mut terminals: HashMap<&str, Vec<&Event>> = HashMap::new();
        for e in &h.events {
            let names_a_request = matches!(e.kind.as_str(), "decision" | "error" | "call" | "resp");
            if names_a_request && !arrivals.contains_key(req_of(e)) {
                out.push(Violation {
                    invariant: "I2",
                    subject: req_of(e).to_string(),
                    at: e.t,
                    detail: format!("`{}` names a request that never arrived", e.kind),
                });
            }
            if e.kind == "decision" || e.kind == "error" {
                terminals.entry(req_of(e)).or_default().push(e);
            }
        }
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
            if end.t < rq.t {
                out.push(Violation {
                    invariant: "I2",
                    subject: id.to_string(),
                    at: end.t,
                    detail: format!("ended {} ms before the request arrived", rq.t - end.t),
                });
            }
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
        // Provenance comes from the history, never from the `origin` label: a cache that stored a
        // fallback records it as an ordinary entry. So every hit must equal the latest model answer
        // for its key that came before it. A history must therefore start with an empty cache.
        let mut model_plan: HashMap<String, Plan> = HashMap::new();
        let mut out = Vec::new();
        for (e, text) in decisions_with_text(h) {
            let id = req_of(e);
            let Some(key) = cache_key(e, text) else {
                continue;
            };
            let Some(plan) = plan_of(e) else { continue };
            let mut flag = |detail: String| {
                out.push(Violation {
                    invariant: "I3",
                    subject: id.to_string(),
                    at: e.t,
                    detail,
                })
            };
            match e.get("source") {
                Some("model") => {
                    model_plan.insert(key, plan);
                }
                Some("cache") => match model_plan.get(&key) {
                    None => flag("cache hit with no earlier model answer for its key".into()),
                    Some(prev) if *prev != plan => {
                        flag("cache served a plan that differs from the latest model answer".into())
                    }
                    Some(_) => {
                        if let Some(origin) = e.get("origin").filter(|o| *o != "model") {
                            flag(format!("cache says it served a {origin} plan"));
                        }
                    }
                },
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
        let mut out = Vec::new();
        for (e, text) in decisions_with_text(h) {
            let id = req_of(e);
            let (Some(text), Some(plan)) = (text, plan_of(e)) else {
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
        let cap = match limits(h) {
            Ok((_, cap)) => cap,
            Err(e) => return bad_meta("I5", e),
        };
        let mut calls: HashMap<&str, (u64, u64)> = HashMap::new();
        // A call without a known request is reported by I2, not counted here.
        let known: std::collections::HashSet<&str> =
            h.of_kind("req").filter_map(|e| e.get("id")).collect();
        for e in h.of_kind("call").filter(|e| known.contains(req_of(e))) {
            let entry = calls.entry(req_of(e)).or_insert((0, e.t));
            entry.0 += 1;
            entry.1 = e.t;
        }
        let mut over: Vec<_> = calls.into_iter().filter(|(_, (n, _))| *n > cap).collect();
        // Ties broken by id: iteration order of a HashMap must never reach the output.
        over.sort_by_key(|(id, (_, last))| (*last, *id));
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
    fn unreadable_or_conflicting_limits_are_errors_not_defaults() {
        let mut h = History::new();
        assert_eq!(limits(&h), Ok((8000, 2)), "absent means default");
        h.add_meta("deadline_ms", "8s");
        assert!(limits(&h).is_err());
        let mut h = History::new();
        h.add_meta("deadline_ms", 99_999_999);
        h.add_meta("deadline_ms", 8000);
        assert!(limits(&h).is_err(), "first-wins would let a file pick its bar");
        h.push(req(0, "r1", "x"));
        h.push(dec(60_000, "r1", "model", 200, 2, 1));
        assert_eq!(failing(&h, false), vec!["I2", "I5"]);
    }

    #[test]
    fn i5_output_order_is_the_same_every_time() {
        let mut events = Vec::new();
        for id in ["ra", "rb", "rc", "rd", "re", "rf"] {
            events.push(req(0, id, "x"));
            for n in 1..=3 {
                events.push(Event::new(5 * n, "call").with("req", id).with("n", n));
            }
        }
        let h = history(events);
        let first = check(&default_set(false), &h).render();
        for _ in 0..20 {
            assert_eq!(check(&default_set(false), &h).render(), first);
        }
    }

    #[test]
    fn a_history_can_declare_its_own_pace_menu() {
        let mut h = history(vec![req(0, "r1", "x"), dec(500, "r1", "model", 100, 2, 1)]);
        assert_eq!(failing(&h, false), vec!["I1"], "100 is off the model's menu");
        h.add_meta("menu_wpm", "100,150,200");
        assert!(failing(&h, false).is_empty());
        h.meta.retain(|(k, _)| k != "menu_wpm");
        h.add_meta("menu_wpm", "fast");
        assert_eq!(failing(&h, false), vec!["I1"]);
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

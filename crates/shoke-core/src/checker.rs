//! Invariants read a [`History`] and return violations. They never see the system that
//! produced it, so the same checkers work on a simulated run, a recorded run or a file.

use crate::history::History;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub invariant: &'static str,
    /// What broke: usually a request or task id.
    pub subject: String,
    /// Virtual time of the offending event, in milliseconds.
    pub at: u64,
    pub detail: String,
}

pub trait Invariant {
    /// Short stable id such as `I4`.
    fn id(&self) -> &'static str;
    /// Kebab-case name shown in reports.
    fn name(&self) -> &'static str;
    fn check(&self, history: &History) -> Vec<Violation>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvariantResult {
    pub id: &'static str,
    pub name: &'static str,
    pub violations: Vec<Violation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub results: Vec<InvariantResult>,
}

/// How many violations are printed per invariant before the rest are summarised.
const SHOWN: usize = 5;

impl Report {
    pub fn passed(&self) -> bool {
        self.results.iter().all(|r| r.violations.is_empty())
    }

    /// Ids of the invariants that failed, in check order.
    pub fn failing(&self) -> Vec<&'static str> {
        self.results
            .iter()
            .filter(|r| !r.violations.is_empty())
            .map(|r| r.id)
            .collect()
    }

    pub fn violations_of(&self, id: &str) -> &[Violation] {
        self.results
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.violations.as_slice())
            .unwrap_or(&[])
    }

    pub fn render(&self) -> String {
        self.render_up_to(SHOWN)
    }

    /// As [`Report::render`], showing up to `shown` violations per invariant.
    pub fn render_up_to(&self, shown: usize) -> String {
        let mut out = String::new();
        for r in &self.results {
            let verdict = if r.violations.is_empty() {
                "PASS".to_string()
            } else {
                format!("FAIL ({})", r.violations.len())
            };
            out.push_str(&format!("{:<4}{:<28}{}\n", r.id, r.name, verdict));
            for v in r.violations.iter().take(shown) {
                out.push_str(&format!("      {} @{}ms  {}\n", v.subject, v.at, v.detail));
            }
            if r.violations.len() > shown {
                out.push_str(&format!(
                    "      ...and {} more\n",
                    r.violations.len() - shown
                ));
            }
        }
        out.push_str(&format!(
            "verdict: {}\n",
            if self.passed() { "PASS" } else { "FAIL" }
        ));
        out
    }
}

/// Invariants always see events in time order. A file written by anything need not be in it, and
/// a verdict must not depend on line order. The sort is stable, so same-instant events keep theirs.
pub fn check(invariants: &[Box<dyn Invariant>], history: &History) -> Report {
    let sorted;
    let history = if history.events.windows(2).all(|w| w[0].t <= w[1].t) {
        history
    } else {
        let mut h = history.clone();
        h.sort();
        sorted = h;
        &sorted
    };
    Report {
        results: invariants
            .iter()
            .map(|inv| InvariantResult {
                id: inv.id(),
                name: inv.name(),
                violations: inv.check(history),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Event;

    struct NoBad;
    impl Invariant for NoBad {
        fn id(&self) -> &'static str {
            "T1"
        }
        fn name(&self) -> &'static str {
            "no-bad-events"
        }
        fn check(&self, h: &History) -> Vec<Violation> {
            h.of_kind("bad")
                .map(|e| Violation {
                    invariant: "T1",
                    subject: e.get("id").unwrap_or("?").to_string(),
                    at: e.t,
                    detail: "a bad event".into(),
                })
                .collect()
        }
    }

    fn invs() -> Vec<Box<dyn Invariant>> {
        vec![Box::new(NoBad)]
    }

    #[test]
    fn passing_history_passes() {
        let mut h = History::new();
        h.push(Event::new(1, "ok"));
        let r = check(&invs(), &h);
        assert!(r.passed());
        assert!(r.render().contains("verdict: PASS"));
        assert!(r.failing().is_empty());
    }

    #[test]
    fn failing_history_names_the_subject() {
        let mut h = History::new();
        h.push(Event::new(9, "bad").with("id", "r7"));
        let r = check(&invs(), &h);
        assert!(!r.passed());
        assert_eq!(r.failing(), vec!["T1"]);
        assert_eq!(r.violations_of("T1")[0].subject, "r7");
        assert!(r.render().contains("r7 @9ms"));
    }

    /// Fails when the first event it sees is not the earliest one.
    struct FirstIsEarliest;
    impl Invariant for FirstIsEarliest {
        fn id(&self) -> &'static str {
            "T2"
        }
        fn name(&self) -> &'static str {
            "first-is-earliest"
        }
        fn check(&self, h: &History) -> Vec<Violation> {
            let min = h.events.iter().map(|e| e.t).min().unwrap_or(0);
            match h.events.first() {
                Some(e) if e.t != min => vec![Violation {
                    invariant: "T2",
                    subject: e.kind.clone(),
                    at: e.t,
                    detail: "saw events out of time order".into(),
                }],
                _ => Vec::new(),
            }
        }
    }

    #[test]
    fn invariants_see_events_in_time_order_whatever_the_file_order() {
        let mut h = History::new();
        h.push(Event::new(20, "late"));
        h.push(Event::new(5, "early"));
        let invs: Vec<Box<dyn Invariant>> = vec![Box::new(FirstIsEarliest)];
        assert!(check(&invs, &h).passed());
    }

    #[test]
    fn long_lists_are_summarised() {
        let mut h = History::new();
        for i in 0..8 {
            h.push(Event::new(i, "bad").with("id", format!("r{i}")));
        }
        let r = check(&invs(), &h);
        assert!(r.render().contains("...and 3 more"));
        assert!(!r.render_up_to(usize::MAX).contains("more"));
        assert!(r.render_up_to(usize::MAX).contains("r7 @7ms"));
    }
}

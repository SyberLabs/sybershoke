//! Reduce a failing fault list to a minimal one.
//!
//! Two passes, both driven by a caller-supplied predicate `fails(&[F]) -> bool` that re-runs the
//! scenario:
//!
//! 1. **Remove.** Delta debugging (ddmin) drops chunks of faults, then single faults, until every
//!    remaining fault is needed.
//! 2. **Simplify.** Each surviving fault is offered its `simpler()` variants (shorter, earlier,
//!    weaker) and keeps any that still fail. This is the time-aware step: a stall that lasted
//!    29 000 ms may fail just as well at 250 ms.
//!
//! `simpler()` must strictly reduce some measure, otherwise the second pass would not terminate.
//! `max_runs` bounds the total work regardless.

pub trait Simplify: Sized + Clone {
    /// Strictly simpler variants of `self`, most aggressive first. Empty when `self` is minimal.
    fn simpler(&self) -> Vec<Self>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shrunk<F> {
    pub faults: Vec<F>,
    /// How many times the predicate was called.
    pub runs: u32,
    /// False when the input did not fail to begin with; `faults` is then returned unchanged.
    pub reproduced: bool,
}

pub fn shrink<F: Simplify>(
    faults: Vec<F>,
    max_runs: u32,
    mut fails: impl FnMut(&[F]) -> bool,
) -> Shrunk<F> {
    let mut faults = faults;
    let mut runs = 0u32;

    macro_rules! fails_now {
        ($c:expr) => {{
            runs += 1;
            fails($c)
        }};
    }

    if !fails_now!(&faults) {
        return Shrunk {
            faults,
            runs,
            reproduced: false,
        };
    }
    let empty: Vec<F> = Vec::new();
    if !faults.is_empty() && fails_now!(&empty) {
        return Shrunk {
            faults: empty,
            runs,
            reproduced: true,
        };
    }

    // Pass 1: ddmin over the fault list.
    let mut n = 2usize;
    while faults.len() >= 2 && runs < max_runs {
        let len = faults.len();
        let chunk = len.div_ceil(n);
        let mut reduced = false;
        let mut start = 0;
        while start < len && runs < max_runs {
            let end = (start + chunk).min(len);
            let mut candidate = Vec::with_capacity(len - (end - start));
            candidate.extend_from_slice(&faults[..start]);
            candidate.extend_from_slice(&faults[end..]);
            if !candidate.is_empty() && fails_now!(&candidate) {
                faults = candidate;
                n = n.saturating_sub(1).max(2);
                reduced = true;
                break;
            }
            start = end;
        }
        if !reduced {
            if n >= len {
                break;
            }
            n = (n * 2).min(len);
        }
    }

    // Pass 2: make each surviving fault simpler, until nothing changes.
    let mut changed = true;
    while changed && runs < max_runs {
        changed = false;
        for i in 0..faults.len() {
            for candidate in faults[i].simpler() {
                if runs >= max_runs {
                    break;
                }
                let mut trial = faults.clone();
                trial[i] = candidate;
                if fails_now!(&trial) {
                    faults = trial;
                    changed = true;
                    break;
                }
            }
        }
    }

    Shrunk {
        faults,
        runs,
        reproduced: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct F {
        id: u32,
        dur: u64,
    }

    impl Simplify for F {
        fn simpler(&self) -> Vec<F> {
            let mut v = Vec::new();
            if self.dur > 100 {
                v.push(F {
                    id: self.id,
                    dur: (self.dur / 2).max(100),
                });
            }
            v
        }
    }

    fn f(id: u32, dur: u64) -> F {
        F { id, dur }
    }

    #[test]
    fn finds_the_single_culprit() {
        let faults: Vec<F> = (0..20).map(|i| f(i, 1000)).collect();
        let out = shrink(faults, 1000, |c| c.iter().any(|x| x.id == 13));
        assert!(out.reproduced);
        assert_eq!(out.faults.len(), 1);
        assert_eq!(out.faults[0].id, 13);
    }

    #[test]
    fn finds_a_needed_pair() {
        let faults: Vec<F> = (0..16).map(|i| f(i, 1000)).collect();
        let out = shrink(faults, 1000, |c| {
            c.iter().any(|x| x.id == 3) && c.iter().any(|x| x.id == 11)
        });
        let ids: Vec<u32> = out.faults.iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![3, 11]);
    }

    #[test]
    fn shortens_durations_down_to_the_threshold() {
        // Fails only while some fault lasts at least 400 ms.
        let out = shrink(vec![f(1, 29_000), f(2, 500)], 1000, |c| {
            c.iter().any(|x| x.dur >= 400)
        });
        assert_eq!(out.faults.len(), 1);
        assert!(out.faults[0].dur >= 400 && out.faults[0].dur < 800);
    }

    #[test]
    fn empty_set_is_returned_when_faults_do_not_matter() {
        let out = shrink(vec![f(1, 1000), f(2, 1000)], 100, |_| true);
        assert!(out.faults.is_empty());
        assert!(out.reproduced);
    }

    #[test]
    fn non_reproducing_input_is_returned_unchanged() {
        let input = vec![f(1, 1000)];
        let out = shrink(input.clone(), 100, |_| false);
        assert!(!out.reproduced);
        assert_eq!(out.faults, input);
        assert_eq!(out.runs, 1);
    }

    #[test]
    fn respects_the_run_budget() {
        let faults: Vec<F> = (0..64).map(|i| f(i, 1_000_000)).collect();
        let out = shrink(faults, 10, |c| c.iter().any(|x| x.id == 40));
        assert!(out.runs <= 12, "runs = {}", out.runs);
    }
}

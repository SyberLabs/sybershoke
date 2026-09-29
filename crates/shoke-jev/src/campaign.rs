//! Many seeds at once: how often is each invariant broken, and what is the smallest set of
//! faults that breaks it?
//!
//! The counts answer the question the acceptance-key finding raised: a bug can pass every
//! campaign that injects faults together and still be real. Sweeping a fault *mix* one class at
//! a time shows which class finds which bug.

use crate::faults::{JevFault, Mix};
use crate::sim::{simulate, Config, Scenario};
use shoke_core::{check, shrink, Invariant, Shrunk};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct SweepOpts {
    pub start: u64,
    pub seeds: u64,
    pub requests: usize,
    pub faults: usize,
    pub mix: Mix,
}

#[derive(Clone, Debug, Default)]
pub struct SweepResult {
    pub seeds: u64,
    /// Invariant id -> number of seeds on which it failed.
    pub failing_seeds: BTreeMap<&'static str, u64>,
    /// Invariant id -> the first (lowest) failing seed.
    pub first_seed: BTreeMap<&'static str, u64>,
}

impl SweepResult {
    pub fn count(&self, id: &str) -> u64 {
        self.failing_seeds.get(id).copied().unwrap_or(0)
    }
}

pub fn sweep(cfg: &Config, opts: &SweepOpts, invariants: &[Box<dyn Invariant>]) -> SweepResult {
    let mut result = SweepResult {
        seeds: opts.seeds,
        ..SweepResult::default()
    };
    // Exactly `opts.seeds` seeds, even near the top of the range: never fewer than reported.
    for seed in (0..opts.seeds).map(|i| opts.start.wrapping_add(i)) {
        let sc = Scenario::generate(seed, opts.requests, opts.faults, opts.mix);
        let report = check(invariants, &simulate(cfg, &sc));
        for id in report.failing() {
            *result.failing_seeds.entry(id).or_insert(0) += 1;
            result.first_seed.entry(id).or_insert(seed);
        }
    }
    result
}

/// The smallest fault list, and shortest windows, that still break `invariant_id`.
pub fn minimal(
    cfg: &Config,
    scenario: &Scenario,
    invariants: &[Box<dyn Invariant>],
    invariant_id: &str,
    max_runs: u32,
) -> Shrunk<JevFault> {
    shrink(scenario.faults.clone(), max_runs, |faults| {
        let sc = scenario.with_faults(faults.to_vec());
        check(invariants, &simulate(cfg, &sc))
            .failing()
            .contains(&invariant_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invariants::default_set;
    use crate::provider::Profile;
    use crate::sim::Bug;

    fn opts() -> SweepOpts {
        SweepOpts {
            start: 0,
            seeds: 40,
            requests: 60,
            faults: 2,
            mix: Mix::All,
        }
    }

    #[test]
    fn a_correct_pipeline_passes_every_seed() {
        let mut cfg = Config::new(Profile::Jev);
        cfg.floor = true;
        let r = sweep(&cfg, &opts(), &default_set(true));
        assert!(r.failing_seeds.is_empty(), "{:?}", r.failing_seeds);
    }

    #[test]
    fn each_seeded_bug_is_caught_by_its_target_invariant() {
        for bug in Bug::ALL {
            let mut cfg = Config::new(Profile::Jev);
            cfg.floor = matches!(bug, Bug::CacheFallback);
            cfg.bugs.insert(bug);
            let r = sweep(&cfg, &opts(), &default_set(false));
            assert!(
                r.count(bug.target()) > 0,
                "{} was not caught by {}: {:?}",
                bug.name(),
                bug.target(),
                r.failing_seeds
            );
        }
    }

    #[test]
    fn a_fault_free_bug_shrinks_to_no_faults() {
        let mut cfg = Config::new(Profile::Jev);
        cfg.bugs.insert(Bug::BareKeywordOverride);
        let invs = default_set(false);
        let sc = Scenario::generate(1, 60, 3, Mix::All);
        let out = minimal(&cfg, &sc, &invs, "I4", 200);
        assert!(out.reproduced);
        assert!(out.faults.is_empty(), "{:?}", out.faults);
    }

    #[test]
    fn a_fault_dependent_bug_shrinks_to_a_small_fault_set() {
        let mut cfg = Config::new(Profile::Jev);
        cfg.bugs.insert(Bug::RetryStorm);
        let invs = default_set(false);
        let first = sweep(
            &cfg,
            &SweepOpts {
                mix: Mix::Http,
                ..opts()
            },
            &invs,
        );
        let seed = *first
            .first_seed
            .get("I5")
            .expect("retry storm is caught by http faults");
        let sc = Scenario::generate(seed, 60, 2, Mix::Http);
        let out = minimal(&cfg, &sc, &invs, "I5", 500);
        assert!(out.reproduced);
        assert_eq!(out.faults.len(), 1, "{:?}", out.faults);
        assert!(out.faults[0].dur <= sc.faults.iter().map(|f| f.dur).max().unwrap());
    }
}

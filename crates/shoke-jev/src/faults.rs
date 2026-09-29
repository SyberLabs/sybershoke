//! Provider faults, injected at the model client: the place a fault proxy would sit between the
//! Worker and the model host. A fault is active for provider calls that *start* inside its
//! window `[at, at + dur)`.

use shoke_core::{Rng, Simplify};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// The provider answers with an error status (429, 500 or 503) after a short delay.
    Http(u16),
    /// The provider is slow: this many extra milliseconds.
    Slow(u64),
    /// A cold start: the call takes about 35 seconds (the Kev README's scale-to-zero figure).
    ColdStart,
    /// The answer arrives cut off and cannot be parsed.
    Truncate,
    /// The answer parses but names a value that is not on the menu.
    OutOfMenu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JevFault {
    pub at: u64,
    pub dur: u64,
    pub kind: FaultKind,
}

impl JevFault {
    pub fn active(&self, t: u64) -> bool {
        t >= self.at && t < self.at + self.dur
    }
}

impl fmt::Display for JevFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            FaultKind::Http(c) => format!("http-{c}"),
            FaultKind::Slow(x) => format!("slow-{x}"),
            FaultKind::ColdStart => "cold-start".to_string(),
            FaultKind::Truncate => "truncate".to_string(),
            FaultKind::OutOfMenu => "out-of-menu".to_string(),
        };
        write!(f, "at={},dur={},kind={}", self.at, self.dur, kind)
    }
}

/// Smallest duration a shrunk fault is reduced to.
const MIN_DUR: u64 = 250;

impl Simplify for JevFault {
    /// Shorter, earlier, weaker. Each candidate strictly lowers `dur`, `at` or the slow amount,
    /// which is what guarantees the shrinker terminates.
    fn simpler(&self) -> Vec<Self> {
        let mut out = Vec::new();
        if self.dur > MIN_DUR {
            let mut short = self.clone();
            short.dur = MIN_DUR;
            out.push(short);
            let half = (self.dur / 2).max(MIN_DUR);
            if half != MIN_DUR {
                let mut f = self.clone();
                f.dur = half;
                out.push(f);
            }
        }
        if self.at > 0 {
            let mut f = self.clone();
            f.at = self.at / 2;
            out.push(f);
        }
        if let FaultKind::Slow(x) = self.kind {
            if x > 1000 {
                let mut f = self.clone();
                f.kind = FaultKind::Slow(x / 2);
                out.push(f);
            }
        }
        out
    }
}

/// Which fault classes a campaign draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mix {
    All,
    Http,
    Slow,
    Cold,
    Truncate,
    OutOfMenu,
}

impl Mix {
    pub fn parse(s: &str) -> Option<Mix> {
        Some(match s {
            "all" => Mix::All,
            "http" => Mix::Http,
            "slow" => Mix::Slow,
            "cold" => Mix::Cold,
            "truncate" => Mix::Truncate,
            "menu" | "out-of-menu" => Mix::OutOfMenu,
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Mix::All => "all",
            Mix::Http => "http",
            Mix::Slow => "slow",
            Mix::Cold => "cold",
            Mix::Truncate => "truncate",
            Mix::OutOfMenu => "menu",
        }
    }
}

/// `n` faults with start times spread over `horizon` milliseconds.
pub fn generate(rng: &mut Rng, n: usize, horizon: u64, mix: Mix) -> Vec<JevFault> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let class = match mix {
            Mix::All => rng.below(5),
            Mix::Http => 0,
            Mix::Slow => 1,
            Mix::Cold => 2,
            Mix::Truncate => 3,
            Mix::OutOfMenu => 4,
        };
        let kind = match class {
            0 => {
                let codes = [429u16, 503, 500];
                FaultKind::Http(*rng.pick(&codes))
            }
            1 => FaultKind::Slow(2000 + rng.below(20_000)),
            2 => FaultKind::ColdStart,
            3 => FaultKind::Truncate,
            _ => FaultKind::OutOfMenu,
        };
        let at = rng.below(horizon.max(1));
        let dur = 1000 + rng.below(15_000);
        out.push(JevFault { at, dur, kind });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_half_open() {
        let f = JevFault {
            at: 100,
            dur: 50,
            kind: FaultKind::Truncate,
        };
        assert!(!f.active(99));
        assert!(f.active(100));
        assert!(f.active(149));
        assert!(!f.active(150));
    }

    #[test]
    fn simpler_strictly_shrinks() {
        let mut f = JevFault {
            at: 9000,
            dur: 12_000,
            kind: FaultKind::Slow(20_000),
        };
        // Repeatedly taking the first candidate must terminate.
        for _ in 0..200 {
            match f.simpler().into_iter().next() {
                Some(next) => f = next,
                None => return,
            }
        }
        panic!("simplification did not terminate: {f}");
    }

    #[test]
    fn minimal_fault_has_no_simpler_form() {
        let f = JevFault {
            at: 0,
            dur: MIN_DUR,
            kind: FaultKind::Http(503),
        };
        assert!(f.simpler().is_empty());
    }

    #[test]
    fn generation_is_deterministic_and_respects_mix() {
        let a = generate(&mut Rng::new(5), 6, 100_000, Mix::All);
        let b = generate(&mut Rng::new(5), 6, 100_000, Mix::All);
        assert_eq!(a, b);
        let only = generate(&mut Rng::new(5), 10, 100_000, Mix::Http);
        assert!(only.iter().all(|f| matches!(f.kind, FaultKind::Http(_))));
    }

    #[test]
    fn display_is_stable() {
        let f = JevFault {
            at: 1,
            dur: 2,
            kind: FaultKind::Http(503),
        };
        assert_eq!(f.to_string(), "at=1,dur=2,kind=http-503");
    }
}

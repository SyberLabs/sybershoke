//! Request traffic. Mostly short gaps; occasionally a gap of two to three minutes, which is
//! what lets a scale-to-zero host go cold without any injected fault.

use shoke_core::Rng;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub at: u64,
    pub text: String,
}

const BASES: [&str; 20] = [
    "tokyo drift",
    "drift off to sleep",
    "slow and quiet",
    "epic battle",
    "no visuals just read",
    "fast and funny",
    "night drive",
    "calm rain",
    "neon racing",
    "help me sleep",
    "silent reading",
    "fast racing",
    "something relaxing",
    "tokyo drift but slow",
    "racing game energy",
    "quiet evening",
    "the neon city",
    "just read, no visuals",
    "fast and loud",
    "drift",
];

const SUFFIXES: [&str; 11] = [
    "",
    "",
    "",
    " please",
    ", slow",
    " and quiet",
    " with no visuals",
    " fast",
    ", silent",
    " tonight",
    " for an hour",
];

/// Case and spacing variations of the same request. They normalise to the same cache key.
fn vary(rng: &mut Rng, text: &str) -> String {
    match rng.below(4) {
        0 => text.to_string(),
        1 => text.to_uppercase(),
        2 => text.replace(' ', "  "),
        _ => format!("{}!", text.trim()),
    }
}

fn fresh(rng: &mut Rng) -> String {
    let base = *rng.pick(&BASES);
    let suffix = *rng.pick(&SUFFIXES);
    vary(rng, &format!("{base}{suffix}"))
}

pub fn generate(rng: &mut Rng, n: usize) -> Vec<Request> {
    let mut out: Vec<Request> = Vec::with_capacity(n);
    let mut t = 500u64;
    for i in 0..n {
        let text = if !out.is_empty() && rng.chance(1, 4) {
            let previous = rng.pick(&out).text.clone();
            vary(rng, &previous)
        } else {
            fresh(rng)
        };
        out.push(Request {
            id: format!("r{}", i + 1),
            at: t,
            text,
        });
        t += if rng.chance(1, 12) {
            130_000 + rng.below(70_000)
        } else {
            200 + rng.below(3_000)
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::normalize;
    use std::collections::HashSet;

    #[test]
    fn deterministic_ordered_and_sized() {
        let a = generate(&mut Rng::new(9), 50);
        let b = generate(&mut Rng::new(9), 50);
        assert_eq!(a, b);
        assert_eq!(a.len(), 50);
        assert!(a.windows(2).all(|w| w[0].at < w[1].at));
        assert_eq!(a[0].id, "r1");
    }

    #[test]
    fn repeats_share_a_cache_key() {
        let reqs = generate(&mut Rng::new(3), 200);
        let keys: HashSet<String> = reqs.iter().map(|r| normalize(&r.text)).collect();
        assert!(keys.len() < reqs.len(), "some requests must repeat");
    }

    #[test]
    fn idle_gaps_exist() {
        let reqs = generate(&mut Rng::new(4), 200);
        assert!(reqs.windows(2).any(|w| w[1].at - w[0].at > 120_000));
    }
}

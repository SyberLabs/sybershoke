//! The model host, as the Worker sees it: a call takes some time and returns a plan, an error,
//! or something unusable. [`call`] is the fault proxy.

use crate::faults::{FaultKind, JevFault};
use crate::menu::{Plan, AMBIENT, BOOKS, FRACTAL, NEON, SYNTH};
use crate::text::{expectations, normalize, requests_night_drive, Expect, NightDrive};
use shoke_core::Rng;

/// Kev's documented scale-to-zero cold start (Kev README, via the local-GPU report).
pub const COLD_START_MS: u64 = 35_000;
/// **Assumption.** How long a scale-to-zero host sits idle before it is torn down. The project
/// docs give the cold-start cost but not this threshold. Change it here and rerun the report.
pub const IDLE_COLD_MS: u64 = 120_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Jev through a hosted API: steady, slower per call.
    Jev,
    /// Kev on a warm GPU: fast per call.
    KevWarm,
    /// Kev on scale-to-zero: fast when warm, ~35 s on the first call after idle.
    KevScaleToZero,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Profile> {
        Some(match s {
            "jev" => Profile::Jev,
            "kev-warm" => Profile::KevWarm,
            "kev-cold" | "kev-scale-to-zero" => Profile::KevScaleToZero,
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Profile::Jev => "jev",
            Profile::KevWarm => "kev-warm",
            Profile::KevScaleToZero => "kev-cold",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ok(Plan),
    Http(u16),
    Truncated,
    OutOfMenu(Plan),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub latency: u64,
    pub kind: Kind,
}

fn fnv(s: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Push a plan toward what the request explicitly asks for.
pub fn apply_expectations(p: &mut Plan, text: &str) {
    for rule in expectations(text) {
        match rule.expect {
            Expect::WpmAtLeast(n) => {
                if p.wpm < n {
                    p.wpm = 300;
                }
            }
            Expect::WpmAtMost(n) => {
                if p.wpm > n {
                    p.wpm = 150;
                }
            }
            Expect::LoudnessAtMost(n) => {
                if p.sound > n {
                    p.sound = n;
                }
            }
            Expect::LoudnessAtLeast(n) => {
                if p.sound < n {
                    p.sound = n;
                }
            }
            Expect::VisualOff => p.visual = 0,
        }
    }
}

/// A well-behaved model: understands the night-drive reference and honours explicit words.
pub fn model_answer(text: &str) -> Plan {
    let mut p = Plan {
        wpm: 200,
        sound: AMBIENT,
        visual: FRACTAL,
        book: (fnv(&normalize(text)) % BOOKS as u64) as u8,
    };
    if requests_night_drive(text, NightDrive::Narrow) {
        p.wpm = 300;
        p.sound = SYNTH;
        p.visual = NEON;
    }
    apply_expectations(&mut p, text);
    p
}

/// The preset floor: a keyword-to-preset mapper that needs no model. It picks book 0 and
/// otherwise follows the same explicit words. (The recommended floor in the local-GPU report.)
pub fn preset_floor(text: &str) -> Plan {
    let mut p = Plan {
        wpm: 200,
        sound: AMBIENT,
        visual: FRACTAL,
        book: 0,
    };
    if requests_night_drive(text, NightDrive::Narrow) {
        p.wpm = 300;
        p.sound = SYNTH;
        p.visual = NEON;
    }
    apply_expectations(&mut p, text);
    p
}

fn corrupt(mut p: Plan, rng: &mut Rng) -> Plan {
    match rng.below(4) {
        0 => p.wpm = 999,
        1 => p.sound = 9,
        2 => p.visual = 7,
        _ => p.book = 40,
    }
    p
}

/// The model host's warmth. A scale-to-zero host goes cold after [`IDLE_COLD_MS`] without a call,
/// and a call that arrives while it boots waits for the boot to finish.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Host {
    /// Start of the previous call.
    pub last_call: Option<u64>,
    /// When the current boot finishes.
    pub ready_at: u64,
}

impl Host {
    pub fn warm_at(t: u64) -> Host {
        Host {
            last_call: Some(t),
            ready_at: t,
        }
    }
}

/// One provider call starting at `at`.
pub fn call(
    profile: Profile,
    faults: &[JevFault],
    text: &str,
    at: u64,
    host: &mut Host,
    rng: &mut Rng,
) -> Response {
    let mut latency = match profile {
        Profile::Jev => 600 + rng.below(1900),
        Profile::KevWarm | Profile::KevScaleToZero => 60 + rng.below(340),
    };
    if profile == Profile::KevScaleToZero {
        if at < host.ready_at {
            latency += host.ready_at - at;
        } else if host
            .last_call
            .is_none_or(|last| at.saturating_sub(last) > IDLE_COLD_MS)
        {
            latency = COLD_START_MS + rng.below(3000);
            host.ready_at = at + latency;
        }
    }
    host.last_call = Some(at);

    let mut kind = Kind::Ok(model_answer(text));
    for f in faults.iter().filter(|f| f.active(at)) {
        match f.kind {
            FaultKind::Http(code) => {
                return Response {
                    latency: 30 + rng.below(70),
                    kind: Kind::Http(code),
                };
            }
            FaultKind::Slow(extra) => latency += extra,
            FaultKind::ColdStart => latency = latency.max(COLD_START_MS + rng.below(3000)),
            FaultKind::Truncate => kind = Kind::Truncated,
            FaultKind::OutOfMenu => kind = Kind::OutOfMenu(corrupt(model_answer(text), rng)),
        }
    }
    Response { latency, kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_answers_are_valid_and_honour_the_request() {
        for t in [
            "tokyo drift",
            "drift off to sleep",
            "slow and quiet",
            "just read, no visuals",
            "fast and loud",
            "silent reading",
        ] {
            let p = model_answer(t);
            assert!(p.is_valid(), "{t}: {p:?}");
            for rule in expectations(t) {
                assert!(rule.satisfied_by(&p), "{t}: {}", rule.explain(&p));
            }
        }
    }

    #[test]
    fn floor_honours_the_request_too() {
        for t in ["drift off to sleep", "fast", "silent", "no visuals"] {
            let p = preset_floor(t);
            for rule in expectations(t) {
                assert!(rule.satisfied_by(&p), "{t}");
            }
        }
    }

    #[test]
    fn tokyo_drift_gets_the_night_drive_look() {
        let p = model_answer("tokyo drift");
        assert_eq!((p.wpm, p.sound, p.visual), (300, SYNTH, NEON));
    }

    #[test]
    fn http_fault_fails_fast() {
        let faults = [JevFault {
            at: 0,
            dur: 1000,
            kind: FaultKind::Http(503),
        }];
        let mut last = Host::default();
        let r = call(Profile::Jev, &faults, "x", 10, &mut last, &mut Rng::new(1));
        assert_eq!(r.kind, Kind::Http(503));
        assert!(r.latency < 200);
        // Outside the window the call succeeds.
        let r = call(
            Profile::Jev,
            &faults,
            "x",
            5000,
            &mut last,
            &mut Rng::new(1),
        );
        assert!(matches!(r.kind, Kind::Ok(_)));
    }

    #[test]
    fn scale_to_zero_goes_cold_after_idle_only() {
        let mut last = Host::default();
        let first = call(
            Profile::KevScaleToZero,
            &[],
            "x",
            1000,
            &mut last,
            &mut Rng::new(1),
        );
        assert!(first.latency >= COLD_START_MS, "first call is cold");
        let warm = call(
            Profile::KevScaleToZero,
            &[],
            "x",
            45_000,
            &mut last,
            &mut Rng::new(1),
        );
        assert!(warm.latency < 1000, "once booted, the next call is warm");
        let cold = call(
            Profile::KevScaleToZero,
            &[],
            "x",
            45_000 + IDLE_COLD_MS + 1,
            &mut last,
            &mut Rng::new(1),
        );
        assert!(cold.latency >= COLD_START_MS, "after idle it is cold again");
    }

    #[test]
    fn a_call_during_a_cold_start_waits_for_it() {
        let mut host = Host::warm_at(0);
        let at = IDLE_COLD_MS + 10_000;
        let first = call(
            Profile::KevScaleToZero,
            &[],
            "x",
            at,
            &mut host,
            &mut Rng::new(1),
        );
        assert!(first.latency >= COLD_START_MS);
        // 1.6 s later the host is still booting: the answer cannot come before it is ready.
        let second = call(
            Profile::KevScaleToZero,
            &[],
            "y",
            at + 1600,
            &mut host,
            &mut Rng::new(2),
        );
        assert!(
            at + 1600 + second.latency >= at + first.latency,
            "{second:?}"
        );
    }

    #[test]
    fn out_of_menu_answers_are_invalid() {
        let faults = [JevFault {
            at: 0,
            dur: 1000,
            kind: FaultKind::OutOfMenu,
        }];
        let mut last = Host::default();
        let r = call(Profile::Jev, &faults, "x", 1, &mut last, &mut Rng::new(3));
        match r.kind {
            Kind::OutOfMenu(p) => assert!(!p.is_valid()),
            other => panic!("unexpected {other:?}"),
        }
    }
}

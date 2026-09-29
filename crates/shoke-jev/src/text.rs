//! Reading a request the way a careful person would.
//!
//! Two consumers share these rules on purpose. The *checker* uses [`expectations`] as its
//! oracle: "slow" must not get a fast plan, "silent" must be silent. The *model* uses them to
//! stand in for a well-behaved Jev or Kev. The bugs under test live in the pipeline around the
//! model (the override, the cache, the deadline), not in the model itself, so the stand-in is
//! deliberately correct.
//!
//! These are the "rule verifiers" the post-training report names as the most trusted labels
//! (fast means at least 250 wpm, silence means silent audio, no visuals means visuals off).

use crate::menu::Plan;

/// Lower-case alphanumeric words. Punctuation and case never matter.
pub fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// The cache key: words joined by single spaces.
pub fn normalize(s: &str) -> String {
    words(s).join(" ")
}

fn negated(ws: &[String], i: usize) -> bool {
    i > 0
        && matches!(
            ws[i - 1].as_str(),
            "not" | "no" | "never" | "dont" | "t" | "without"
        )
}

fn has_any(ws: &[String], list: &[&str]) -> bool {
    ws.iter()
        .enumerate()
        .any(|(i, w)| list.contains(&w.as_str()) && !negated(ws, i))
}

fn has_seq(ws: &[String], seq: &[&str]) -> bool {
    !seq.is_empty()
        && ws
            .windows(seq.len())
            .any(|win| win.iter().zip(seq).all(|(a, b)| a == b))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    WpmAtLeast(u32),
    WpmAtMost(u32),
    LoudnessAtMost(u8),
    VisualOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// The words in the request that created this expectation.
    pub cause: &'static str,
    pub expect: Expect,
}

impl Rule {
    pub fn satisfied_by(&self, p: &Plan) -> bool {
        match self.expect {
            Expect::WpmAtLeast(n) => p.wpm >= n,
            Expect::WpmAtMost(n) => p.wpm <= n,
            Expect::LoudnessAtMost(n) => p.sound <= n,
            Expect::VisualOff => p.visual == 0,
        }
    }

    /// A one-line explanation of a violation, for reports.
    pub fn explain(&self, p: &Plan) -> String {
        match self.expect {
            Expect::WpmAtLeast(n) => {
                format!("\"{}\" needs wpm >= {}, got {}", self.cause, n, p.wpm)
            }
            Expect::WpmAtMost(n) => format!("\"{}\" needs wpm <= {}, got {}", self.cause, n, p.wpm),
            Expect::LoudnessAtMost(n) => format!(
                "\"{}\" needs sound rank <= {} ({}), got {} ({})",
                self.cause,
                n,
                crate::menu::SOUNDS[n as usize],
                p.sound,
                crate::menu::SOUNDS
                    .get(p.sound as usize)
                    .copied()
                    .unwrap_or("?")
            ),
            Expect::VisualOff => format!(
                "\"{}\" needs visuals off, got visual={}",
                self.cause, p.visual
            ),
        }
    }
}

/// What an explicit request obliges any correct plan to do. Conflicting pace words ("fast" and
/// "slow" together) produce no pace expectation rather than a wrong one.
pub fn expectations(text: &str) -> Vec<Rule> {
    let ws = words(text);
    let mut rules = Vec::new();

    let fast = has_any(&ws, &["fast", "quick", "rapid"]);
    let slow = has_any(
        &ws,
        &[
            "slow", "slowly", "sleep", "sleepy", "calm", "relax", "relaxing", "gentle",
        ],
    );
    if fast && !slow {
        rules.push(Rule {
            cause: "fast",
            expect: Expect::WpmAtLeast(250),
        });
    }
    if slow && !fast {
        rules.push(Rule {
            cause: "slow/sleep/calm",
            expect: Expect::WpmAtMost(200),
        });
    }

    let silent = has_any(&ws, &["silent", "silence", "mute"])
        || has_seq(&ws, &["no", "sound"])
        || has_seq(&ws, &["without", "sound"]);
    let quiet = has_any(&ws, &["quiet", "quietly", "soft", "hushed"]);
    if silent {
        rules.push(Rule {
            cause: "silent",
            expect: Expect::LoudnessAtMost(0),
        });
    } else if quiet {
        rules.push(Rule {
            cause: "quiet",
            expect: Expect::LoudnessAtMost(1),
        });
    }

    let no_visuals = has_seq(&ws, &["no", "visuals"])
        || has_seq(&ws, &["no", "visual"])
        || has_seq(&ws, &["without", "visuals"])
        || has_seq(&ws, &["just", "read"])
        || has_seq(&ws, &["text", "only"]);
    if no_visuals {
        rules.push(Rule {
            cause: "no visuals",
            expect: Expect::VisualOff,
        });
    }
    rules
}

/// Which keyword rule decides that a request is "night drive".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NightDrive {
    /// A phrase (`tokyo drift`, `night drive`), and nothing that asks for calm or silence.
    /// This is the narrowing the post-training report proposes.
    Narrow,
    /// Any of the bare words `racing`, `neon`, `tokyo`, `drift`, whatever surrounds them. The
    /// report says the real `requestsNightDrive` misfires on "drift off to sleep"; this is the
    /// modelled shape of that misfire, not a copy of the code.
    Bare,
}

pub fn requests_night_drive(text: &str, mode: NightDrive) -> bool {
    let ws = words(text);
    match mode {
        NightDrive::Bare => ws
            .iter()
            .any(|w| matches!(w.as_str(), "racing" | "neon" | "tokyo" | "drift")),
        NightDrive::Narrow => {
            let phrase = has_seq(&ws, &["tokyo", "drift"]) || has_seq(&ws, &["night", "drive"]);
            let blocked = ws.iter().any(|w| {
                matches!(
                    w.as_str(),
                    "slow"
                        | "slowly"
                        | "quiet"
                        | "quietly"
                        | "calm"
                        | "sleep"
                        | "sleepy"
                        | "silent"
                        | "silence"
                        | "relax"
                        | "relaxing"
                        | "gentle"
                        | "no"
                        | "not"
                        | "without"
                )
            });
            phrase && !blocked
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn causes(t: &str) -> Vec<Expect> {
        expectations(t).iter().map(|r| r.expect).collect()
    }

    #[test]
    fn normalize_ignores_case_space_and_punctuation() {
        assert_eq!(normalize("  Tokyo   DRIFT!! "), "tokyo drift");
        assert_eq!(normalize("tokyo drift"), normalize("TOKYO, drift."));
    }

    #[test]
    fn pace_rules() {
        assert_eq!(causes("fast and funny"), vec![Expect::WpmAtLeast(250)]);
        assert_eq!(causes("drift off to sleep"), vec![Expect::WpmAtMost(200)]);
        assert!(
            causes("fast but slow").is_empty(),
            "conflict gives no pace rule"
        );
        assert!(causes("not slow").is_empty(), "negated word is ignored");
    }

    #[test]
    fn sound_and_visual_rules() {
        assert_eq!(causes("silent reading"), vec![Expect::LoudnessAtMost(0)]);
        assert_eq!(causes("quiet evening"), vec![Expect::LoudnessAtMost(1)]);
        assert_eq!(causes("just read, no visuals"), vec![Expect::VisualOff]);
        assert_eq!(causes("no sound"), vec![Expect::LoudnessAtMost(0)]);
        assert!(causes("epic battle").is_empty());
    }

    #[test]
    fn night_drive_narrow_rejects_the_misfires() {
        assert!(requests_night_drive("tokyo drift", NightDrive::Narrow));
        assert!(requests_night_drive("Night  Drive", NightDrive::Narrow));
        assert!(!requests_night_drive(
            "drift off to sleep",
            NightDrive::Narrow
        ));
        assert!(!requests_night_drive(
            "tokyo drift but slow",
            NightDrive::Narrow
        ));
        assert!(!requests_night_drive(
            "tokyo drift, no visuals",
            NightDrive::Narrow
        ));
        assert!(!requests_night_drive("neon racing", NightDrive::Narrow));
    }

    #[test]
    fn night_drive_bare_reproduces_the_misfire() {
        assert!(requests_night_drive("drift off to sleep", NightDrive::Bare));
        assert!(requests_night_drive("neon racing", NightDrive::Bare));
        assert!(!requests_night_drive("epic battle", NightDrive::Bare));
    }

    #[test]
    fn rule_explanations_name_the_cause() {
        let p = Plan {
            wpm: 300,
            sound: 3,
            visual: 3,
            book: 0,
        };
        let r = expectations("drift off to sleep");
        assert!(!r[0].satisfied_by(&p));
        assert!(r[0].explain(&p).contains("wpm <= 200, got 300"));
    }
}

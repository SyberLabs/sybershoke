//! An oracle the checker does not share. `golden/phrases.tsv` is labelled by hand; these tests
//! grade both the checker's rules and the "correct" pipeline against it, which is what breaks the
//! circularity of I4 using the same function the model uses.

use shoke_core::check;
use shoke_jev::menu::{Plan, AMBIENT};
use shoke_jev::sim::{simulate, Config, Scenario};
use shoke_jev::text::{expectations, Expect};
use shoke_jev::workload::Request;
use shoke_jev::{default_set, Profile};
use std::path::PathBuf;

/// Phrases where the checker demands less than a person would. Safe (no false violation), but a
/// pipeline that ignores these words passes I4. Kept by name so the gap stays visible.
const KNOWN_GAPS: &[&str] = &["quick nap before bed"];

fn labels() -> Vec<(String, Vec<String>)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/phrases.tsv");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (text, labels) = l.split_once('\t').expect("text<TAB>labels");
            let labels = labels
                .split(',')
                .filter(|s| *s != "-")
                .map(str::to_string)
                .collect();
            (text.to_string(), labels)
        })
        .collect()
}

fn label_of(e: Expect) -> &'static str {
    match e {
        Expect::WpmAtLeast(_) => "fast",
        Expect::WpmAtMost(_) => "slow",
        Expect::LoudnessAtMost(0) => "silent",
        Expect::LoudnessAtMost(_) => "quiet",
        Expect::LoudnessAtLeast(_) => "loud",
        Expect::VisualOff => "novisuals",
    }
}

fn satisfies(label: &str, p: &Plan) -> bool {
    match label {
        "fast" => p.wpm >= 250,
        "slow" => p.wpm <= 200,
        "silent" => p.sound == 0,
        "quiet" => p.sound <= 1,
        "loud" => p.sound >= AMBIENT,
        "novisuals" => p.visual == 0,
        other => panic!("unknown label {other}"),
    }
}

#[test]
fn the_checker_never_demands_what_a_person_would_not() {
    for (text, want) in labels() {
        for rule in expectations(&text) {
            let got = label_of(rule.expect);
            assert!(want.iter().any(|w| w == got), "{text:?}: checker demands {got}, labels say {want:?}");
        }
    }
}

#[test]
fn the_checker_demands_everything_a_person_would_except_the_named_gaps() {
    for (text, want) in labels() {
        let got: Vec<&str> = expectations(&text).iter().map(|r| label_of(r.expect)).collect();
        let missing: Vec<&String> = want.iter().filter(|w| !got.contains(&w.as_str())).collect();
        let known = KNOWN_GAPS.contains(&text.as_str());
        assert_eq!(missing.is_empty(), !known, "{text:?}: missing {missing:?} (known gap: {known})");
    }
}

#[test]
fn the_correct_pipeline_honours_the_hand_labels() {
    for (text, want) in labels() {
        let sc = Scenario {
            seed: 1,
            requests: vec![Request { id: "r1".into(), at: 500, text: text.clone() }],
            faults: Vec::new(),
        };
        let mut cfg = Config::new(Profile::Jev);
        cfg.floor = true;
        let h = simulate(&cfg, &sc);
        assert!(check(&default_set(true), &h).passed(), "{text:?}");
        let d = h.of_kind("decision").next().expect("one decision");
        let plan = Plan {
            wpm: d.get_u64("wpm").unwrap() as u32,
            sound: d.get_u64("sound").unwrap() as u8,
            visual: d.get_u64("visual").unwrap() as u8,
            book: d.get_u64("book").unwrap() as u8,
        };
        for w in &want {
            assert!(satisfies(w, &plan), "{text:?}: plan {plan:?} breaks the hand label {w}");
        }
    }
}

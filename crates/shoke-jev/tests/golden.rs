//! Week 1 exit test: every hand-written history gives the expected verdict, and names the
//! offending request. The files are the specification of what each invariant means.

use shoke_core::{check, History};
use shoke_jev::default_set;
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden")
}

struct Case {
    name: String,
    text: String,
    expect: Vec<String>,
    require_floor: bool,
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(golden_dir()).expect("golden dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("hist") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let mut expect = None;
        let mut require_floor = false;
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("# expect:") {
                let v = v.trim();
                expect = Some(if v == "none" {
                    Vec::new()
                } else {
                    v.split_whitespace().map(str::to_string).collect()
                });
            }
            if line.trim() == "# flags: require-floor" {
                require_floor = true;
            }
        }
        out.push(Case {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            text,
            expect: expect
                .unwrap_or_else(|| panic!("{}: missing `# expect:` line", path.display())),
            require_floor,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[test]
fn every_golden_history_gives_its_expected_verdict() {
    let cases = cases();
    assert!(
        cases.len() >= 8,
        "golden set shrank to {} files",
        cases.len()
    );
    for c in &cases {
        let h = History::from_text(&c.text).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        let report = check(&default_set(c.require_floor), &h);
        let got: Vec<String> = report.failing().iter().map(|s| s.to_string()).collect();
        assert_eq!(got, c.expect, "{}:\n{}", c.name, report.render());
    }
}

#[test]
fn violations_name_the_offending_request() {
    for c in cases().iter().filter(|c| !c.expect.is_empty()) {
        let h = History::from_text(&c.text).unwrap();
        let report = check(&default_set(c.require_floor), &h);
        for id in &c.expect {
            let vs = report.violations_of(id);
            assert!(!vs.is_empty(), "{}: {id} should fail", c.name);
            assert!(
                vs.iter().all(|v| v.subject.starts_with('r')),
                "{}: subject should be a request id: {vs:?}",
                c.name
            );
        }
    }
}

#[test]
fn golden_files_survive_a_round_trip() {
    for c in cases() {
        let h = History::from_text(&c.text).unwrap();
        let again = History::from_text(&h.to_text()).unwrap();
        assert_eq!(h, again, "{}", c.name);
    }
}

#[test]
fn i7_is_silent_without_the_flag() {
    let case = cases()
        .into_iter()
        .find(|c| c.name == "i7_no_plan.hist")
        .unwrap();
    let h = History::from_text(&case.text).unwrap();
    assert!(check(&default_set(false), &h).passed());
}

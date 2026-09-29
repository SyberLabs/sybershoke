//! The binary, end to end: exit codes, and the round trip from `run --out` to `check`.

use std::path::PathBuf;
use std::process::{Command, Output};

fn shoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shoke"))
        .args(args)
        .output()
        .expect("run shoke")
}

fn golden(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../shoke-jev/golden")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("exit code")
}

fn temp_file(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("shoke-test-{}-{name}", std::process::id()));
    p.to_string_lossy().into_owned()
}

#[test]
fn check_passes_a_clean_history() {
    let o = shoke(&["check", &golden("clean.hist")]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    assert!(stdout(&o).contains("verdict: PASS"));
}

#[test]
fn check_fails_a_bad_history_and_names_the_request() {
    let o = shoke(&["check", &golden("i4_sleep_gets_fast.hist")]);
    assert_eq!(code(&o), 1);
    let out = stdout(&o);
    assert!(out.contains("I4"), "{out}");
    assert!(out.contains("r1 @1300ms"), "{out}");
    assert!(out.contains("wpm <= 200, got 300"), "{out}");
}

#[test]
fn require_floor_turns_on_i7() {
    let path = golden("i7_no_plan.hist");
    assert_eq!(code(&shoke(&["check", &path])), 0);
    assert_eq!(code(&shoke(&["check", &path, "--require-floor"])), 1);
}

#[test]
fn bad_input_is_a_usage_error_not_a_violation() {
    assert_eq!(code(&shoke(&["check", "/no/such/file.hist"])), 2);
    let bad = temp_file("bad.hist");
    std::fs::write(&bad, "not a history\n").unwrap();
    assert_eq!(code(&shoke(&["check", &bad])), 2);
    assert_eq!(code(&shoke(&["run", "--frobnicate"])), 2);
    assert_eq!(code(&shoke(&["run", "--bug", "nope"])), 2);
    assert_eq!(code(&shoke(&["nonsense"])), 2);
    assert_eq!(code(&shoke(&[])), 2);
    let _ = std::fs::remove_file(bad);
}

#[test]
fn a_written_history_checks_to_the_identical_report() {
    let file = temp_file("roundtrip.hist");
    let run = shoke(&[
        "run",
        "--seed",
        "7",
        "--faults",
        "0",
        "--bug",
        "bare-keyword-override",
        "--out",
        &file,
    ]);
    assert_eq!(code(&run), 1);
    let checked = shoke(&["check", &file]);
    assert_eq!(code(&checked), 1);

    // Compare only the invariant lines: the headers legitimately differ.
    let body = |s: &str| -> Vec<String> {
        s.lines()
            .filter(|l| l.starts_with('I') || l.starts_with("      ") || l.starts_with("verdict"))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(body(&stdout(&run)), body(&stdout(&checked)));
    let _ = std::fs::remove_file(file);
}

#[test]
fn a_correct_pipeline_exits_zero_under_faults() {
    let o = shoke(&["sweep", "--floor", "--require-floor", "--seeds", "30"]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
}

#[test]
fn sweep_finds_and_shrinks_a_seeded_bug() {
    let o = shoke(&[
        "sweep",
        "--bug",
        "retry-storm",
        "--mix",
        "http",
        "--seeds",
        "60",
        "--shrink",
    ]);
    assert_eq!(code(&o), 1);
    let out = stdout(&o);
    assert!(out.contains("I5"), "{out}");
    assert!(out.contains("shrunk:"), "{out}");
    assert!(out.contains("-> 1 after"), "{out}");
}

#[test]
fn shrink_command_rejects_a_seed_that_does_not_fail() {
    let o = shoke(&["shrink", "--seed", "1", "--faults", "0"]);
    assert_eq!(code(&o), 2);
}

#[test]
fn same_seed_same_output() {
    let a = shoke(&["run", "--seed", "99", "--bug", "no-deadline"]);
    let b = shoke(&["run", "--seed", "99", "--bug", "no-deadline"]);
    assert_eq!(stdout(&a), stdout(&b));
}

#[test]
fn bugs_lists_all_six() {
    let o = shoke(&["bugs"]);
    assert_eq!(code(&o), 0);
    assert_eq!(stdout(&o).lines().count(), 6);
}

#[test]
fn inputs_that_would_pass_vacuously_are_usage_errors() {
    for args in [
        &["sweep", "--seeds", "0"][..],
        &["run", "--requests", "0"],
        &["sweep", "--seed", "18446744073709551615", "--seeds", "2"],
        &["run", "5", "--seed", "1"],
        &["report", "--seeds", "0"],
    ] {
        let o = shoke(args);
        assert_eq!(code(&o), 2, "{args:?}: {}", stdout(&o));
    }
    // The last seed of the range is still usable on its own.
    let o = shoke(&["sweep", "--seed", "18446744073709551615", "--seeds", "1"]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
}

#[test]
fn check_rejects_histories_it_cannot_judge_and_names_its_bar() {
    let cases = [
        ("empty", "shoke-history/v1\n"),
        ("bad-deadline", "shoke-history/v1\nmeta deadline_ms=8s\n0 req id=r1 text=x\n"),
        (
            "two-deadlines",
            "shoke-history/v1\nmeta deadline_ms=99999999\nmeta deadline_ms=8000\n0 req id=r1 text=x\n",
        ),
    ];
    for (name, text) in cases {
        let f = temp_file(name);
        std::fs::write(&f, text).unwrap();
        assert_eq!(code(&shoke(&["check", &f])), 2, "{name}");
        let _ = std::fs::remove_file(f);
    }
    let o = shoke(&["check", &golden("clean.hist")]);
    assert!(
        stdout(&o).contains("deadline_ms=8000 max_calls=2"),
        "{}",
        stdout(&o)
    );
}

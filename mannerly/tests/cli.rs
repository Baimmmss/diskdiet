//! End-to-end tests: run the real binary against small fixture CLIs.
#![cfg(unix)]

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

fn mannerly(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mannerly"))
        .args(args)
        .env_remove("NO_COLOR")
        .output()
        .expect("run mannerly")
}

fn fixture(name: &str) -> String {
    let p: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    p.to_string_lossy().into_owned()
}

fn report(args: &[&str]) -> Value {
    let out = mannerly(args);
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn status<'a>(r: &'a Value, id: &str) -> &'a str {
    r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no check {id}"))["status"]
        .as_str()
        .unwrap()
}

#[test]
fn polite_tool_scores_100() {
    let r = report(&["--json", &fixture("polite.sh")]);
    let failing: Vec<_> = r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["status"] == "fail" || c["status"] == "warn")
        .map(|c| format!("{}: {}", c["id"], c["detail"]))
        .collect();
    assert_eq!(r["score"], 100, "{failing:#?}");
}

#[test]
fn rude_tool_gets_caught() {
    let r = report(&["--json", &fixture("rude.sh")]);
    for id in [
        "errors.exit-code",
        "errors.no-traceback",
        "output.pipe-color",
        "robust.no-litter",
        "help.usage",
        "help.examples",
    ] {
        assert_eq!(status(&r, id), "fail", "{id}");
    }
    assert_eq!(status(&r, "help.version"), "warn");
    assert!(r["score"].as_u64().unwrap() < 60, "score {}", r["score"]);
}

#[test]
fn ignoring_ctrl_c_fails() {
    let r = report(&["--json", "--timeout", "2", &fixture("stubborn.sh")]);
    assert_eq!(status(&r, "robust.ctrl-c"), "fail");
    assert_eq!(status(&r, "robust.no-hang"), "warn");
    assert_eq!(status(&r, "robust.not-silent"), "warn");
}

#[test]
fn no_bare_skips_bare_runs() {
    let r = report(&["--json", "--no-bare", &fixture("stubborn.sh")]);
    for id in ["robust.ctrl-c", "robust.no-hang", "robust.not-silent"] {
        assert_eq!(status(&r, id), "skip", "{id}");
    }
}

#[test]
fn mannerly_has_good_manners() {
    let r = report(&["--json", env!("CARGO_BIN_EXE_mannerly")]);
    assert_eq!(r["score"], 100, "{:#}", r["checks"]);
}

#[test]
fn min_score_sets_exit_code() {
    let rude = fixture("rude.sh");
    let out = mannerly(&["--quiet", "--min-score", "90", &rude]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("below --min-score 90"));
    let ok = mannerly(&["--quiet", "--min-score", "90", &fixture("polite.sh")]);
    assert_eq!(ok.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ok.stdout).trim(), "100");
}

#[test]
fn writes_badge_and_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let badge = dir.path().join("manners.svg");
    let out = mannerly(&[
        "--markdown",
        "--badge",
        badge.to_str().unwrap(),
        &fixture("rude.sh"),
    ]);
    assert!(out.status.success());
    let md = String::from_utf8_lossy(&out.stdout);
    assert!(md.starts_with("### mannerly:"), "{md}");
    assert!(md.contains("errors.exit-code"));
    let svg = std::fs::read_to_string(&badge).unwrap();
    assert!(svg.starts_with("<svg") && svg.contains("/100"));
}

#[test]
fn missing_command_is_a_clear_error() {
    let out = mannerly(&["definitely-not-a-real-command-xyz"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("command not found"), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(mannerly(&[]).status.code(), Some(2));
    assert_eq!(mannerly(&["--bogus"]).status.code(), Some(2));
}

#[test]
fn unknown_check_ids_are_rejected() {
    let out = mannerly(&["--skip", "nope", "true"]);
    assert_eq!(out.status.code(), Some(1));
    let out = mannerly(&["--explain", "nope"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn explain_and_list() {
    let out = mannerly(&["--explain", "output.no-color"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("NO_COLOR"));
    let list = mannerly(&["--list-checks"]);
    let text = String::from_utf8_lossy(&list.stdout);
    assert!(text.contains("robust.ctrl-c") && text.contains("help.flag"));
}

#[test]
fn trailing_args_belong_to_the_target() {
    // `sh -c ...` gets `--help` etc. appended; mannerly must not parse `-c` itself.
    let r = report(&["--json", "sh", "-c", "echo hi", "x"]);
    assert_eq!(r["command"], "sh -c echo hi x");
}

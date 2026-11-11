//! End-to-end tests that run the real binary against a throwaway folder.

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn diskdiet(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diskdiet"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("binary runs")
}

fn write(p: &Path, len: usize) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, vec![b'x'; len]).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    write(&r.join("web/package.json"), 10);
    write(&r.join("web/node_modules/left-pad/index.js"), 40_000);
    write(&r.join("cli/Cargo.toml"), 10);
    write(&r.join("cli/target/release/cli"), 90_000);
    write(&r.join("docs/build/index.html"), 5_000); // no marker: must survive
    t
}

fn json(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("valid JSON")
}

#[test]
fn json_lists_artifacts_biggest_first() {
    let t = fixture();
    let v = json(&diskdiet(&[t.path().to_str().unwrap(), "--json"]));
    let entries = v["entries"].as_array().unwrap();
    let paths: Vec<&str> = entries
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert_eq!(entries.len(), 2, "{paths:?}");
    assert!(paths[0].ends_with("target"));
    assert!(paths[1].ends_with("node_modules"));
    assert_eq!(entries[0]["ecosystem"], "rust");
    assert_eq!(entries[0]["kind"], "project");
    assert!(v["total_bytes"].as_u64().unwrap() >= 130_000);
}

#[test]
fn filters_by_ecosystem_size_and_age() {
    let t = fixture();
    let root = t.path().to_str().unwrap();
    let only = json(&diskdiet(&[root, "--json", "--only", "js"]));
    assert_eq!(only["entries"].as_array().unwrap().len(), 1);
    assert_eq!(only["entries"][0]["ecosystem"], "node");

    let excl = json(&diskdiet(&[root, "--json", "--exclude", "rust,node"]));
    assert!(excl["entries"].as_array().unwrap().is_empty());

    let big = json(&diskdiet(&[root, "--json", "--min-size", "1GB"]));
    assert!(big["entries"].as_array().unwrap().is_empty());

    // everything was just created, so nothing is 30 days old
    let old = json(&diskdiet(&[root, "--json", "--older-than", "30d"]));
    assert!(old["entries"].as_array().unwrap().is_empty());
}

#[test]
fn list_output_is_a_table() {
    let t = fixture();
    let out = diskdiet(&[t.path().to_str().unwrap(), "--list"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(text.contains("SIZE"));
    assert!(text.contains("node_modules"));
    assert!(text.contains("in 2 folders"));
    assert!(!text.contains("\x1b["), "no colors when piped");
}

#[test]
fn delete_needs_confirmation_without_a_terminal() {
    let t = fixture();
    let out = diskdiet(&[t.path().to_str().unwrap(), "--delete"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(t.path().join("web/node_modules").exists());
}

#[test]
fn delete_yes_removes_only_matches() {
    let t = fixture();
    let r = t.path();
    let out = diskdiet(&[r.to_str().unwrap(), "--delete", "--yes", "--only", "node"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Freed"));
    assert!(!r.join("web/node_modules").exists());
    assert!(r.join("web/package.json").exists());
    assert!(r.join("cli/target").exists());
    assert!(r.join("docs/build/index.html").exists());
}

#[test]
fn rejects_bad_input() {
    let out = diskdiet(&["--only", "cobol", "--json"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown ecosystem"));

    let out = diskdiet(&["/definitely/not/here", "--json"]);
    assert!(!out.status.success());

    let out = diskdiet(&["--older-than", "soon"]);
    assert!(!out.status.success());
}

#[test]
fn lists_ecosystems() {
    let out = diskdiet(&["--ecosystems"]);
    let text = String::from_utf8(out.stdout).unwrap();
    for eco in ["node", "rust", "python", "xcode", "unity"] {
        assert!(text.lines().any(|l| l == eco), "missing {eco}");
    }
}

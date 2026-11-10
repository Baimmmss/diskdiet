//! Non-interactive output: an aligned table and JSON.

use std::path::Path;
use std::time::UNIX_EPOCH;

use serde_json::json;

use crate::caches;
use crate::scan::{Entry, Source};
use crate::units;

/// `/Users/me/code/x` → `~/code/x`.
pub fn tilde(path: &Path) -> String {
    if let Some(home) = caches::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        };
    }
    path.display().to_string()
}

pub fn table(entries: &[Entry], color: bool) {
    let paint = |code: &str, s: String| {
        if color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s
        }
    };
    if entries.is_empty() {
        println!("Nothing to clean up. Your disk is already on a diet 🥗");
        return;
    }
    println!(
        "{}",
        paint(
            "1",
            format!("{:>9}  {:>5}  {:<10}  {}", "SIZE", "AGE", "TYPE", "PATH")
        )
    );
    for e in entries {
        let size = format!("{:>9}", units::bytes(e.bytes));
        let size = match e.bytes {
            b if b >= 1_000_000_000 => paint("1;31", size),
            b if b >= 100_000_000 => paint("33", size),
            _ => size,
        };
        let age = match e.source {
            Source::Project => units::short_age(e.last_modified),
            Source::Cache => "cache".into(),
        };
        println!(
            "{size}  {}  {:<10}  {}",
            paint("2", format!("{age:>5}")),
            e.ecosystem,
            tilde(&e.path)
        );
    }
    let total: u64 = entries.iter().map(|e| e.bytes).sum();
    println!(
        "\n{} in {} folders",
        paint("1;32", units::bytes(total)),
        entries.len()
    );
}

pub fn json(entries: &[Entry]) -> String {
    let items: Vec<_> = entries
        .iter()
        .map(|e| {
            let modified = e
                .last_modified
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            json!({
                "path": e.path,
                "kind": match e.source { Source::Project => "project", Source::Cache => "cache" },
                "ecosystem": e.ecosystem,
                "what": e.what,
                "restore": e.restore,
                "bytes": e.bytes,
                "files": e.files,
                "last_modified": modified,
                "age_days": units::age(e.last_modified).map(|a| a.as_secs() / 86_400),
            })
        })
        .collect();
    let total: u64 = entries.iter().map(|e| e.bytes).sum();
    serde_json::to_string_pretty(&json!({ "total_bytes": total, "entries": items }))
        .expect("serialisable")
}

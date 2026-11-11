//! Parallel filesystem walk that streams matches as soon as they are sized.

use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use rayon::prelude::*;

use crate::{caches, rules};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Project,
    Cache,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub source: Source,
    pub ecosystem: &'static str,
    pub what: &'static str,
    pub restore: &'static str,
    pub bytes: u64,
    pub files: u64,
    /// For projects: the newest edit to a project file outside any artifact
    /// folder. For caches: when the cache directory last changed.
    pub last_modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
pub struct Progress {
    pub dirs: AtomicU64,
    pub errors: AtomicU64,
    pub done: AtomicBool,
}

pub struct Options {
    pub roots: Vec<PathBuf>,
    pub projects: bool,
    pub caches: bool,
}

/// Directory names that never contain projects worth cleaning but can take a
/// long time to walk (VCS internals, toolchain installs, trash, app bundles).
const SKIP_NAMES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".jj",
    ".Trash",
    ".Trashes",
    ".Spotlight-V100",
    ".fseventsd",
    ".DocumentRevisions-V100",
    ".cache",
    ".rustup",
    ".cargo",
    ".npm",
    ".pnpm-store",
    ".yarn",
    ".bun",
    ".deno",
    ".m2",
    ".gradle",
    ".nuget",
    ".nvm",
    ".pyenv",
    ".rbenv",
    ".asdf",
    ".gem",
    ".conda",
    ".local",
    ".docker",
    ".android",
    ".vscode",
    ".vscode-server",
    ".cursor",
    ".orbstack",
    ".pub-cache",
    ".vscode-insiders",
    ".vscode-oss",
    "anaconda3",
    "miniconda3",
    "miniforge3",
    "site-packages",
    "OrbStack",
];

const SKIP_SUFFIXES: &[&str] = &[".app", ".photoslibrary", ".musiclibrary", ".xcarchive"];

/// Top-level folders in the home directory that hold media or OS data rather
/// than code. Global caches inside `~/Library` are found via [`caches`].
const SKIP_IN_HOME: &[&str] = &[
    "Library",
    "Applications",
    "Pictures",
    "Music",
    "Movies",
    "Public",
    "AppData",
];

struct Ctx<'a> {
    tx: &'a Sender<Entry>,
    progress: &'a Progress,
    home: Option<PathBuf>,
    cache_paths: HashSet<PathBuf>,
}

impl Ctx<'_> {
    fn skip(&self, name: &str, path: &Path, parent: &Path) -> bool {
        SKIP_NAMES.contains(&name)
            || SKIP_SUFFIXES.iter().any(|s| name.ends_with(s))
            || (self.home.as_deref() == Some(parent)
                // Hidden folders right in home (~/.config, ~/.gemini, …) are
                // app data: their node_modules belong to the app, not to you.
                && (name.starts_with('.') || SKIP_IN_HOME.contains(&name)))
            || self.cache_paths.contains(path)
            || is_editor_extensions(name, parent)
            || is_sdk(name, path)
    }
}

/// `~/.vscode/extensions`, `~/.cursor/extensions`, `~/.windsurf/extensions` …
/// Extensions ship their own `node_modules` and virtualenvs; removing them
/// breaks the extension, so the whole folder is off limits.
fn is_editor_extensions(name: &str, parent: &Path) -> bool {
    name == "extensions"
        && parent
            .file_name()
            .is_some_and(|p| p.to_string_lossy().starts_with('.'))
}

/// Toolchain checkouts such as the Flutter SDK contain dozens of packages
/// whose build folders belong to the SDK, not to the user.
fn is_sdk(name: &str, path: &Path) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.contains("flutter") || lower.contains("dart"))
        && (path.join("bin/internal/engine.version").exists()
            || path.join("bin/dart").exists() && path.join("lib/_internal").exists())
}

/// Blocks until the scan is complete, sending every match through `tx`.
pub fn run(opts: &Options, progress: &Progress, tx: &Sender<Entry>) {
    let resolved = caches::resolve();
    let ctx = Ctx {
        tx,
        progress,
        home: caches::home_dir().map(|h| h.canonicalize().unwrap_or(h)),
        cache_paths: resolved.iter().map(|(_, p)| p.clone()).collect(),
    };
    let ctx = &ctx;

    let size_caches = || {
        resolved
            .par_iter()
            .filter(|(_, path)| path.is_dir())
            .for_each(|(cache, path)| {
                let (bytes, files) = dir_size(path);
                if bytes == 0 {
                    return;
                }
                let _ = ctx.tx.send(Entry {
                    path: path.clone(),
                    source: Source::Cache,
                    ecosystem: cache.ecosystem,
                    what: cache.what,
                    restore: cache.restore,
                    bytes,
                    files,
                    last_modified: fs::metadata(path).and_then(|m| m.modified()).ok(),
                });
            })
    };
    let walk_projects = || {
        rayon::scope(|s| {
            for root in &opts.roots {
                let root = root.canonicalize().unwrap_or_else(|_| root.clone());
                s.spawn(move |s| walk(root, s, ctx));
            }
        })
    };

    // Projects first: they are what people care about most, and walking
    // them is quick. Then the caches, which are huge piles of small files.
    // Each phase gets every thread; running them side by side only makes
    // them fight over the disk.
    if opts.projects {
        walk_projects();
    }
    if opts.caches {
        size_caches();
    }
    progress.done.store(true, Ordering::Release);
}

fn walk<'s>(dir: PathBuf, s: &rayon::Scope<'s>, ctx: &'s Ctx<'s>) {
    ctx.progress.dirs.fetch_add(1, Ordering::Relaxed);
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => {
            ctx.progress.errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    let dir = std::sync::Arc::new(dir);
    for entry in rd.flatten() {
        // `file_type` does not follow symlinks, so linked dirs are never entered.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();

        if let Some(rule) = rules::match_dir(&name, &path, &dir) {
            let parent = dir.clone();
            s.spawn(move |_| {
                let (bytes, files) = dir_size(&path);
                if bytes == 0 {
                    return;
                }
                let _ = ctx.tx.send(Entry {
                    path,
                    source: Source::Project,
                    ecosystem: rule.ecosystem,
                    what: rule.what,
                    restore: rule.restore,
                    bytes,
                    files,
                    last_modified: last_activity(&parent),
                });
            });
        } else if !ctx.skip(&name, &path, &dir) {
            s.spawn(move |s| walk(path, s, ctx));
        }
    }
}

/// Estimates when a project was last worked on: the newest modification time
/// of its top-level entries, ignoring artifact folders and Finder litter.
pub fn last_activity(project: &Path) -> Option<SystemTime> {
    fs::read_dir(project)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n != ".DS_Store" && !rules::is_artifact_name(&n)
        })
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
        // A folder holding nothing but artifacts: fall back to its own date.
        .or_else(|| fs::metadata(project).and_then(|m| m.modified()).ok())
}

/// Returns `(bytes on disk, file count)` for a directory tree. Hard-linked
/// files (pnpm, uv, …) are only counted once.
pub fn dir_size(path: &Path) -> (u64, u64) {
    let seen = Mutex::new(HashSet::new());
    size_inner(path, &seen)
}

fn size_inner(path: &Path, seen: &Mutex<HashSet<(u64, u64)>>) -> (u64, u64) {
    let Ok(rd) = fs::read_dir(path) else {
        return (0, 0);
    };
    let mut bytes = 0;
    let mut files = 0;
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            subdirs.push(e.path());
        } else if let Ok(md) = e.metadata() {
            files += 1;
            bytes += disk_usage(&md, seen);
        }
    }
    let (b, f) = subdirs
        .par_iter()
        .map(|d| size_inner(d, seen))
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    (bytes + b, files + f)
}

#[cfg(unix)]
fn disk_usage(md: &Metadata, seen: &Mutex<HashSet<(u64, u64)>>) -> u64 {
    use std::os::unix::fs::MetadataExt;
    if md.nlink() > 1 && !seen.lock().unwrap().insert((md.dev(), md.ino())) {
        return 0;
    }
    // Allocated blocks, not apparent length: this is what deleting frees.
    md.blocks() * 512
}

#[cfg(not(unix))]
fn disk_usage(md: &Metadata, _seen: &Mutex<HashSet<(u64, u64)>>) -> u64 {
    md.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn write(p: &Path, len: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![b'x'; len]).unwrap();
    }

    fn scan(root: &Path) -> Vec<Entry> {
        let (tx, rx) = mpsc::channel();
        let progress = Progress::default();
        let opts = Options {
            roots: vec![root.to_path_buf()],
            projects: true,
            caches: false,
        };
        run(&opts, &progress, &tx);
        drop(tx);
        assert!(progress.done.load(Ordering::Acquire));
        let mut v: Vec<Entry> = rx.into_iter().collect();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        v
    }

    #[test]
    fn finds_artifacts_but_not_lookalikes() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(&r.join("web/package.json"), 10);
        write(&r.join("web/node_modules/react/index.js"), 50_000);
        // nested node_modules must not be reported twice
        write(&r.join("web/node_modules/a/node_modules/b/x.js"), 10_000);
        write(&r.join("crate/Cargo.toml"), 10);
        write(&r.join("crate/target/debug/app"), 80_000);
        write(&r.join("notes/build/keep.txt"), 10_000); // no marker → untouched
        write(&r.join(".git/node_modules/x"), 10_000); // skipped dir
        // editor extensions bundle their own dependencies
        write(&r.join(".vscode-x/extensions/ext/package.json"), 10);
        write(
            &r.join(".vscode-x/extensions/ext/node_modules/a.js"),
            10_000,
        );
        // SDK checkouts are not user projects
        write(&r.join("flutter/bin/internal/engine.version"), 10);
        write(&r.join("flutter/packages/tool/pubspec.yaml"), 10);
        write(&r.join("flutter/packages/tool/.dart_tool/x"), 10_000);

        let found = scan(r);
        let names: Vec<String> = found
            .iter()
            .map(|e| {
                e.path
                    .strip_prefix(r.canonicalize().unwrap())
                    .unwrap()
                    .display()
                    .to_string()
                    .replace('\\', "/") // Windows separators
            })
            .collect();
        assert_eq!(names, vec!["crate/target", "web/node_modules"]);
        assert!(found.iter().all(|e| e.bytes > 0 && e.files > 0));
        assert_eq!(found[1].files, 2);
        assert!(found[0].last_modified.is_some());
    }

    #[test]
    fn hidden_app_folders_in_home_are_skipped() {
        let (tx, _rx) = mpsc::channel();
        let progress = Progress::default();
        let home = PathBuf::from("/home/me");
        let ctx = Ctx {
            tx: &tx,
            progress: &progress,
            home: Some(home.clone()),
            cache_paths: HashSet::new(),
        };
        let skip = |name: &str, parent: &Path| ctx.skip(name, &parent.join(name), parent);
        assert!(skip(".config", &home));
        assert!(skip(".gemini", &home));
        assert!(skip("Library", &home));
        assert!(!skip("code", &home));
        // hidden project folders deeper down are still scanned
        assert!(!skip(".dots", &home.join("experimental")));
        assert!(!skip("Library", &home.join("code")));
    }

    #[test]
    fn project_with_only_artifacts_still_has_an_age() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join(".venv/pyvenv.cfg"), 10);
        assert!(last_activity(t.path()).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_counted_once() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a");
        write(&a, 100_000);
        fs::hard_link(&a, t.path().join("b")).unwrap();
        let (bytes, files) = dir_size(t.path());
        assert_eq!(files, 2);
        let single = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&a).unwrap().blocks() * 512
        };
        assert_eq!(bytes, single);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_dirs_are_not_followed() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(&r.join("real/package.json"), 10);
        write(&r.join("real/node_modules/x.js"), 10_000);
        std::os::unix::fs::symlink(r.join("real"), r.join("link")).unwrap();
        assert_eq!(scan(r).len(), 1);
    }
}

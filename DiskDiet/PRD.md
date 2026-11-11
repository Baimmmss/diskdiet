# PRD: diskdiet

> Find and delete the gigabytes your dev tools leave behind.

| | |
|---|---|
| Version | 0.1.0 |
| Status | published on GitHub (no release tag, not on crates.io) |
| Updated | 2026-11-11 |

## Problem

Every project a developer clones leaves rebuildable clutter behind: `node_modules`, Rust and Maven `target/`, Python virtualenvs, Gradle and Flutter `build/`, Unity `Library/`. Package managers and IDEs add machine-wide caches on top (npm, Homebrew, Gradle, pip, Cargo, Xcode DerivedData). On a typical laptop that adds up to tens of gigabytes; on the author's Mac it was 53 GB. Finding it by hand is slow, and deleting the wrong folder is scary. Existing tools mostly cover one ecosystem (npkill: `node_modules`) or skip the global caches.

## Target users

| Who | Need |
|---|---|
| Developers on laptops with full disks | see what's big and stale, delete it safely in a few keystrokes |
| Polyglot developers | one tool across Node, Rust, Python, Java, Swift, Flutter, Unity and more |
| CI runners and self-hosted build machines | scriptable cleanup (`--delete --yes`, `--older-than`) |

## Goals

- Scan a whole home folder plus global caches in seconds, streaming results as they're found
- Never list something that isn't rebuildable; every row says how to get it back
- Show staleness (when the *project* was last edited) so old work can be cleared in bulk
- Tiny and light: single binary, a few MB of memory, no CPU while idle, no network
- A README first screen and screenshot good enough to earn GitHub stars

## Non-goals

- General disk analysis of personal files (photos, downloads, videos)
- Docker images, simulators' runtimes and system caches that need their own tools to clean
- Running ecosystem cleanup commands (`cargo clean`, `npm cache clean`); diskdiet deletes folders directly
- A GUI app

## Success metrics

| Metric | Target |
|---|---|
| GitHub stars | 500 in the first month after launch (guess, see Open questions) |
| False positives | zero confirmed cases of listing non-rebuildable data |
| Full scan time (home + caches, Intel i7 Mac, warm disk) | under 15 s; projects shown within a few seconds |
| Footprint | binary under 2 MB, peak memory under 10 MB, 0% CPU idle |

## Requirements

### Must

- Detect project artifacts only by folder name **plus** a marker file (e.g. `target/` next to `Cargo.toml`, venv with `pyvenv.cfg` inside); 20+ ecosystems
- Find global caches at known per-OS paths (macOS, Linux, Windows), honoring `CARGO_HOME`, `GOPATH`, `LOCALAPPDATA`
- Never follow symlinks while scanning or deleting; re-check each path is a real directory before removal
- Skip places whose artifacts aren't the user's: `.git` internals, editor extension folders, SDK checkouts (Flutter), toolchain installs, hidden app folders directly in home (`~/.config`, `~/.gemini`, …), app bundles, `~/Library` (caches found by path instead)
- Interactive UI: live results, select, select all, age filter (30 d / 90 d / 1 y), search, sort, open in file manager, per-ecosystem breakdown, background deletion, confirm before delete
- Scriptable: `--list`, `--json`, `--delete` (refuses without a terminal unless `--yes`)
- Filters: `--older-than`, `--min-size`, `--only`, `--exclude`, `--caches`, `--no-caches`, `--caches-only`
- Disk usage from allocated blocks, hard links counted once; delete read-only trees (Go module cache)
- Scan projects first, then caches, each with every thread
- No network access, no telemetry

### Should

- Prebuilt binaries for macOS, Linux and Windows via the release workflow; `install.sh`; `cargo install diskdiet`
- CI on macOS, Linux and Windows (fmt, clippy, tests, MSRV 1.88)
- Contributing guide and issue templates for new ecosystems and false positives
- README screenshot regenerated from the real UI (`cargo test screenshot -- --ignored`)

### Could

- Move to Trash instead of permanent delete (`--trash`)
- Demo GIF (`demo.tape` for vhs is ready)
- Homebrew formula
- User-defined rules in a config file
- More ecosystems: Bazel, Nix, Gleam, Docker build cache

## Platforms

| Platform | Support |
|---|---|
| macOS | full, tested on the author's Mac |
| Linux | supported; only the CI workflow covers it, not yet run on GitHub (see Risks) |
| Windows | supported; sizes use file length, not allocated blocks; only CI covers it |

## Risks

| Risk | Mitigation |
|---|---|
| Deleting something that isn't rebuildable | marker files, skip lists, confirm dialog, `--yes` gate, false-positive issue template; found and fixed real cases (editor extensions, Flutter SDK, `~/.config/opencode`) |
| Deletion is permanent | README warning; Trash support is a Could |
| Competition | npkill (Node only), kondo, cargo-sweep; tw93/Mole (~75k stars per mannerly research notes) overlaps on caches |
| Cold disks make scans slow (21 s measured, 33 s with a bad thread split) | projects first, caches after; streaming results |
| Repo layout after the move into `DiskDiet/` breaks GitHub-side things | listed under Open questions; user handles GitHub |

## Milestones

| Milestone | Status |
|---|---|
| Scanner, rules, global caches, safe deletion | done |
| Interactive UI | done |
| README, screenshot, CI and release workflows, install script, contributing docs | done |
| Tests: 30 passing on macOS (23 unit, 7 end-to-end) | done; after the move, `cargo clean -p diskdiet` fixed stale build paths (2026-11-11) |
| Real-machine testing and fixes (false positives, layout, cursor, speed, idle CPU) | done |
| GitHub publish | done by the user: `Baimmmss/diskdiet`, branch `master` |
| Release tag and binaries | todo, only when the user says so |
| crates.io publish | todo, only when the user says so |

## Open questions

- **Repo layout.** The repo root holds `DiskDiet/` and `mannerly/`. Local fix (2026-11-11, not pushed): CI and release workflows now run in `DiskDiet/`, and CI triggers on `master` instead of `main`. Because `mannerly/Cargo.toml` is in the same repo, `cargo install --git` must name the package (`... diskdiet`). Still open: should the repo be DiskDiet-only (crate at the root)?
- **Build files committed.** `DiskDiet/target/` (12,342 files, ~1.2 GB, ~340 MB compressed) is in pushed commit 6703ca8, so `cargo install --git` downloads all of it. Local fix (2026-11-11, not pushed): `DiskDiet/.gitignore` ignores `target/` and the files are untracked (`git rm --cached`). They stay in history until it is rewritten with a force-push; the user decides.
- **No README on the repo's front page.** GitHub only shows a README at the repo root; `DiskDiet/README.md` isn't displayed there.
- **Broken links.** Fixed locally on 2026-11-11: README screenshot and `install.sh` URLs use `master/DiskDiet/...`, which resolve on GitHub now.
- **GitHub account.** Repo is under `Baimmmss`; the crates.io login shown was `punyebaim`.
- **crates.io.** Publish? `cargo login` not done; the name `diskdiet` was free on 2026-11-11.
- **Star target** of 500 in the first month is a guess.
- **Trash vs permanent delete** as the default.

## Research notes (2026-11-10 to 2026-11-11)

### Measured on the author's Mac (Intel Core i7-1068NG7, 8 threads, 16 GB)

| What | Result |
|---|---|
| Binary / download | 1.4 MB / 0.7 MB tar.gz |
| Peak memory | ~5 MB |
| Full scan (home + caches) | 9–13 s warm, 21 s cold; ~4 threads busy, mostly kernel time |
| Projects only (`--no-caches`) | ~1.3 s |
| Idle CPU in the UI | 0% (was ~1% before redraw-on-change) |
| Found | 53.4 GB in 38 folders (36 GB of it global caches) |

### False positives found and fixed

| Case | Fix |
|---|---|
| `node_modules` and venvs inside editor extensions (`~/.antigravity/extensions`, …) | skip `extensions` under hidden folders |
| Flutter SDK's own `.dart_tool` / `build` folders | skip SDK checkouts |
| `~/.pub-cache` package sources | skip, listed as a global cache instead |
| `~/.config/opencode/node_modules`, `~/.gemini/...` | skip hidden folders directly in home |

### Name check

`diskdiet` was free on crates.io on 2026-11-10.

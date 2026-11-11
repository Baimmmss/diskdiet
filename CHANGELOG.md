# Changelog

## [0.1.1] - 2026-11-11

- Prebuilt binaries for macOS, Linux and Windows on GitHub releases; the install script downloads them in seconds
- `cargo binstall diskdiet` installs the prebuilt binary without compiling
- `cargo install diskdiet` builds about 40% faster (thin LTO, fewer ratatui features)
- Fixed README links and install instructions after the repository move

## [0.1.0] - 2026-11-10

First release.

- Interactive UI with live results while scanning, multi-select, age filter, search, sorting and a per-ecosystem breakdown
- Detects build artifacts and caches across 20+ ecosystems; project folders always need a marker file
- Finds global caches for npm, Yarn, pnpm, Bun, pip, uv, Poetry, Cargo, Go, Gradle, Maven, NuGet, Homebrew, CocoaPods, Xcode, Playwright, Electron, Dart and more
- `--list`, `--json` and `--delete --yes` for scripts
- Filters: `--older-than`, `--min-size`, `--only`, `--exclude`, `--caches-only`, `--no-caches`
- Hard-link-aware disk usage; removes read-only trees such as the Go module cache

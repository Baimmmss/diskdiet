# Contributing to diskdiet

Thanks for helping! The most valuable contributions are **new ecosystems** and **reports of false positives**, i.e. folders diskdiet listed that it shouldn't have.

## Adding a project folder rule

Rules live in [`src/rules.rs`](src/rules.rs). A rule fires only when a directory has the right name **and** a marker file proves which tool created it:

```rust
Rule {
    dirs: &[Exact(".gleam-build")],          // or Prefix("cmake-build-")
    markers: &[Sibling("gleam.toml")],       // any one marker is enough
    ecosystem: "gleam",
    what: "Gleam build output",
    restore: "gleam build",
},
```

Markers can be:

- `Sibling("file")`: a file next to the folder (it can be a nested path like `ProjectSettings/ProjectVersion.txt`)
- `SiblingExt("ext")`: any file with that extension next to the folder
- `Inside("file")`: a file inside the folder (how virtualenvs are detected)

Please **always add a marker** unless the folder name is unique to a single tool (like `__pycache__`), and add a test to the `tests` module at the bottom of the file.

## Adding a global cache

Caches live in [`src/caches.rs`](src/caches.rs) and are looked up by path, once per OS:

```rust
c("node", "Bun install cache", REDOWNLOAD, Any, Home, ".bun/install/cache"),
```

Only add caches whose contents are re-downloaded or rebuilt automatically. No user data, no settings.

## Development

```bash
cargo run -- ~/code            # try it
cargo test                     # unit + end-to-end tests
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo test screenshot -- --ignored   # regenerate assets/screenshot.svg
```

CI runs the tests on macOS, Linux and Windows.

## Releasing

Bump the version in `Cargo.toml`, add a section to `CHANGELOG.md`, then push a tag such as `v0.2.0`. The release workflow builds binaries for every platform and publishes to crates.io.

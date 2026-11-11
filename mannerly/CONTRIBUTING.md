# Contributing

Thanks for helping CLIs get better manners.

## Wrong results

A false failure is the most important kind of bug. Please [open an issue](https://github.com/Baimmmss/mannerly/issues/new?template=false-positive.yml) with the command and the check id. `MANNERLY_DUMP=1 mannerly <command>` prints every run's output and exit status to stderr, which usually shows what went wrong.

## Adding a check

1. Describe it in `DEFS` in `src/checks.rs`: an `id` (`category.name`), a short title phrased as what a well-behaved tool does, a weight, `why` and `fix`.
2. If it needs a new kind of run, add it to `Probes` and `collect` in `src/runner.rs`.
3. Evaluate it in `evaluate` in `src/checks.rs`. Use `Skip` when the check doesn't apply, and `Warn` rather than `Fail` when there are good reasons a tool might behave differently.
4. Add a fixture or extend one in `tests/fixtures/`, and a test in `tests/cli.rs`.
5. Run it against a range of real tools (`sh scripts/leaderboard.sh git gh cargo jq npm python3 …`) and look for false failures before opening a PR.

Weights: 8-10 for things that break scripts and agents, 3-5 for things people notice often, 1-2 for polish.

## Before a PR

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
./target/debug/mannerly ./target/debug/mannerly   # must stay at 100
```

## Regenerating the README assets

```bash
cargo build --release
python3 scripts/screenshot.py assets/screenshot.svg -- pip3
sh scripts/leaderboard.sh git cargo rustc go node npm python3 curl gh jq brew ls grep cat tar make uv pip3
./target/release/mannerly --badge assets/badge.svg -q ./target/release/mannerly
```

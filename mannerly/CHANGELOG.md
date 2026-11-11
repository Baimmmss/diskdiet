# Changelog

## v0.1.0

First release.

- 28 checks in four categories: help, errors, output, robustness.
- Runs the command in a pipe, in a real pseudo-terminal (which answers color and cursor queries) and with a closed output pipe.
- Ctrl-C check that understands REPLs (Ctrl-C, Ctrl-C, Ctrl-D).
- Timeouts that scale with the tool's startup time.
- Terminal, `--json`, `--markdown` and `--quiet` output, plus `--badge` for an SVG badge.
- `--min-score` for CI, `--skip`, `--no-bare`, `--explain`, `--list-checks`.
- GitHub Action and an agent skill for Claude Code, Codex, Cursor and others.

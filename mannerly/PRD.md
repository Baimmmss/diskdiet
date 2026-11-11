# PRD: mannerly

> Lighthouse for command-line tools.

| | |
|---|---|
| Version | 0.1.0 |
| Status | draft |
| Updated | 2026-11-11 |

## Problem

CLIs often behave badly in the ways that hurt scripts, CI and coding agents most: --help that fails, exit code 0 on typos, color codes in pipes, crashes when a pipe closes, prompts that hang without a terminal, Ctrl-C that doesn't work. These guidelines are written down (clig.dev, no-color.org) but rarely checked; the few existing checkers are small smoke tests. Coding agents now write many CLIs, and they get these details wrong.

## Target users

| Who | Need |
|---|---|
| CLI authors (Rust, Go, Python, Node) | a fast way to see what's wrong and how to fix it |
| Coding agents (Claude Code, Codex, Cursor) | machine-readable checks to verify the CLIs they build |
| Maintainers and CI | a score gate and a badge for the README |

## Goals

- Check any CLI in one command, with zero config, in under ~5 seconds for well-behaved tools
- Every finding says what happened, why it matters, and how to fix it
- Be fair: no false failures on legitimate behavior (REPLs, servers, BSD tools)
- Score 100 on itself

## Non-goals

- Testing a CLI's actual functionality or correctness
- Static analysis of source code
- Full Windows support in v0.1 (PTY and closed-pipe checks are skipped there)

## Success metrics

| Metric | Target |
|---|---|
| GitHub stars | 1,000 in the first month after launch |
| Hacker News Show HN | front page |
| Adoption | 50 repos showing the manners badge or using the Action |
| False positives | under 1 confirmed false failure per 20 popular tools |

## Requirements

### Must

- Run the target with --help, -h, --version, an unknown flag, no arguments, a subcommand's --help and a misspelled subcommand
- Run in three modes: pipes (stdin /dev/null), closed stdout, and a real PTY that answers terminal queries
- Run in a temp folder; timeouts scale with startup time; kill the whole process group
- Weighted 0-100 score overall and per category (Help, Errors, Output, Robustness)
- Outputs: terminal report, --json, --markdown, --badge FILE, --quiet
- --min-score N for CI (exit 1 below it); exit 2 for usage errors
- --skip, --timeout, --no-bare, --color, --list-checks, --explain ID
- Never run known-dangerous commands (reboot, shutdown, kill, sudo, ...) with no arguments

### Should

- Agent skill that runs mannerly --json and applies framework-specific fixes
- GitHub Action that posts a job summary
- Prebuilt binaries for macOS and Linux, plus cargo install and an install script

### Could

- Compare mode for several commands at once (leaderboard)
- Hosted leaderboard of popular CLIs
- Windows ConPTY support

## Checks

- **Help:** `help.flag`, `help.stdout`, `help.short`, `help.usage`, `help.examples`, `help.width`, `help.subcommand`, `help.docs`, `help.version`
- **Errors:** `errors.exit-code`, `errors.stderr`, `errors.message`, `errors.concise`, `errors.usage-code`, `errors.suggest`, `errors.no-traceback`
- **Output:** `output.pipe-color`, `output.no-color`, `output.term-dumb`, `output.closed-pipe`, `output.stderr-quiet`, `output.newline`, `output.machine`
- **Robustness:** `robust.startup`, `robust.no-hang`, `robust.ctrl-c`, `robust.not-silent`, `robust.no-litter`

Scoring: pass = full weight, warning = half, fail = 0, skipped = excluded from the total.

## Platforms

| Platform | Support |
|---|---|
| macOS | full |
| Linux | full (needs verification) |
| Windows | partial: PTY and closed-pipe checks skipped |

## Risks

| Risk | Mitigation |
|---|---|
| Running unknown commands with no arguments could do something drastic | temp cwd, stdin /dev/null, a denylist, --no-bare, a README safety section |
| False failures make the tool look wrong on launch day | test against 20+ popular tools before launch; REPL and server cases already handled |
| Small competitors exist (clicheck, cliprobe) | deeper checks, PTY realism, fixes for each finding, badge, Action and agent skill |

## Competitors

| Name | Scope |
|---|---|
| clicheck | help/version/exit-code smoke test |
| cliprobe | pipe and EPIPE edge cases |
| clig.dev | the guidelines themselves, no tool |

## Launch plan

1. README first screen: one-line pitch, demo screenshot, cargo install line
2. Leaderboard of 20+ well-known CLIs in the README (an attention hook)
3. Show HN post, then r/rust and r/commandline
4. Badge and Action, so every adopting repo links back

## Milestones

| Milestone | Status |
|---|---|
| Core checks and reports | done |
| README, skill, Action, CI, release | done |
| Tests (22 passing on macOS); Linux and Windows type-check | done; Linux runtime tests still need CI |
| Publish (only when the user says so) | todo |

## Open questions

- GitHub owner/repo name for URLs (currently assumed Baimmmss/mannerly)
- Whether to publish to crates.io and Homebrew at launch

## Research (2026-11-11)

### Star trends

- GitHub trending (weekly/monthly) is dominated by AI coding-agent tooling: agent skills (mattpocock/skills ~321k, pbakaus/impeccable ~108k, Nutlope/hallmark ~53k, cloudflare/security-audit-skill ~69k).
- Rust CLIs built for agents are rising fast: vercel-labs/agent-browser ~49k, vercel-labs/agent-desktop ~21k (+19k in a month), max-sixty/worktrunk ~12k.
- Local open-source alternatives to paid apps also do well (debpalash/VoiceStudio ~107k, cjpais/Handy ~38k).
- Launch evidence: an arXiv preprint (2511.04453) finds Hacker News score is the strongest predictor of star growth; practitioner guides stress a README first screen with a visual and a one-line install.

### Crowded niches we rejected

| Idea | Why not |
|---|---|
| Secret scanner for agent transcripts | 6+ existing tools (agentleaks, trace-scrub, agent-session-audit, ...) |
| Terminal/TUI driver CLI for agents | coder/agent-tty plus agent-tui, pilotty, tui-use, term-cli, hty |
| Context/token bloat auditor | ccmd, Context Auditor and others |
| AI coding 'Wrapped' cards | 5+ existing repos |
| Session replay to GIF | agent-log-gif and session-replay skill |
| AI slop linter for code | aislop, slopcheck, sloppylint, agent-slop, KarpeSlop |
| AI-vs-human code attribution | git-ai, Agent Blame, GitIntel and more |
| Agent-readiness repo score | agentlint and 5+ others |
| Rust duplicate-code detector | jscpd v5 is already Rust (6.5k stars), plus doppelscan |
| Relaunch diskdiet with AI caches | tw93/Mole (~75k) covers most of it; user asked to keep diskdiet local |
| Terminal design skill | tui-design-skill and others exist; mostly markdown |

### Why mannerly

Black-box CLI UX checking (help, exit codes, pipes, NO_COLOR, Ctrl-C) only has tiny competitors (clicheck, cliprobe). Visual score plus badge spreads by itself, a leaderboard of famous tools makes an HN-friendly launch post, and coding agents now write many CLIs that need checking.

Name check: `mannerly` free on crates.io, npm and Homebrew; `manners` taken on crates.io.

### First leaderboard (macOS, debug build)

BSD ls/cat/grep have no --help or --version, which is why they score low. pip fails output.closed-pipe for real (Python BrokenPipeError).

| Score | Command | Failed checks |
|---:|---|---|
| 100 | `mannerly (itself)` | – |
| 100 | `jq` | – |
| 99 | `cargo` | – |
| 96 | `node` | `help.examples` |
| 96 | `curl` | `help.examples` |
| 96 | `brew` | – |
| 95 | `git` | `help.examples` |
| 95 | `gh` | – |
| 95 | `make` | `help.examples` |
| 95 | `uv` | `help.examples` |
| 94 | `python3` | `help.examples`, `help.width` |
| 93 | `rustc` | `help.examples`, `help.width` |
| 92 | `tar` | `help.usage`, `help.examples` |
| 86 | `grep` | `help.flag` |
| 84 | `pip3` | `help.examples`, `output.closed-pipe`, `robust.startup` |
| 76 | `ls` | `help.flag`, `help.version` |
| 74 | `npm` | `help.flag`, `errors.stderr` |
| 73 | `cat` | `help.flag`, `help.version` |
| 71 | `go` | `help.flag`, `help.subcommand`, `help.version` |

### Sources

- https://github.com/trending?since=weekly
- https://github.com/trending?since=monthly
- https://github.com/trending/rust?since=monthly
- https://arxiv.org/abs/2511.04453v1
- https://github.com/Nutlope/hallmark

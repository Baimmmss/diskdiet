//! The checks: what good command-line manners look like, and how to tell.

use crate::probe::{Outcome, is_crash_signal, signal_name};
use crate::runner::{BOGUS_FLAG, Probes};
use crate::text;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Help,
    Errors,
    Output,
    Robustness,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Help,
        Category::Errors,
        Category::Output,
        Category::Robustness,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Help => "Help",
            Category::Errors => "Errors",
            Category::Output => "Output",
            Category::Robustness => "Robustness",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    /// Not applicable to this tool, or could not be tested here.
    Skip,
}

impl Status {
    fn credit(self) -> f64 {
        match self {
            Status::Pass => 1.0,
            Status::Warn => 0.5,
            Status::Fail | Status::Skip => 0.0,
        }
    }
}

pub struct Def {
    pub id: &'static str,
    pub category: Category,
    pub title: &'static str,
    pub weight: u32,
    pub why: &'static str,
    pub fix: &'static str,
}

pub const DEFS: &[Def] = &[
    // Help
    Def {
        id: "help.flag",
        category: Category::Help,
        title: "--help prints help and exits 0",
        weight: 10,
        why: "--help is the first thing people and coding agents try. If it fails, they give up or guess.",
        fix: "Handle --help before anything else (config loading, network, auth), print to stdout and exit 0.",
    },
    Def {
        id: "help.stdout",
        category: Category::Help,
        title: "Help goes to stdout",
        weight: 5,
        why: "Help that goes to stderr can't be paged or searched: `tool --help | less` and `| grep` see nothing.",
        fix: "Print requested help to stdout. Only errors and diagnostics belong on stderr.",
    },
    Def {
        id: "help.short",
        category: Category::Help,
        title: "-h also shows help",
        weight: 2,
        why: "Many people type -h first. If -h means something else, they get a surprise instead of help.",
        fix: "Make -h an alias for --help, unless -h has a long-standing meaning in your domain.",
    },
    Def {
        id: "help.usage",
        category: Category::Help,
        title: "Help starts with a usage line",
        weight: 3,
        why: "A usage line (`Usage: tool [OPTIONS] <FILE>`) answers \"how do I call this?\" at a glance.",
        fix: "Begin help with `Usage: <name> [OPTIONS] <ARGS>`; most argument parsers can generate it.",
    },
    Def {
        id: "help.examples",
        category: Category::Help,
        title: "Help shows examples",
        weight: 3,
        why: "People and agents copy examples far more than they read option tables.",
        fix: "Add an Examples section with two to five real commands, most common first.",
    },
    Def {
        id: "help.width",
        category: Category::Help,
        title: "Help fits in 100 columns when piped",
        weight: 2,
        why: "Very long lines wrap badly in narrow windows, editors and CI logs.",
        fix: "Wrap help text at 80-100 columns when stdout isn't a terminal.",
    },
    Def {
        id: "help.subcommand",
        category: Category::Help,
        title: "Subcommands have their own --help",
        weight: 3,
        why: "`tool build --help` is how people find a subcommand's options.",
        fix: "Give every subcommand a --help that lists its own arguments and options.",
    },
    Def {
        id: "help.docs",
        category: Category::Help,
        title: "Help links to docs or support",
        weight: 1,
        why: "When help isn't enough, a URL tells people where to go next.",
        fix: "End help with a line like `Docs: https://...` or `Report bugs: https://...`.",
    },
    Def {
        id: "help.version",
        category: Category::Help,
        title: "--version prints a version number",
        weight: 5,
        why: "Bug reports, install scripts and agents all need to know which version is installed.",
        fix: "Support --version: print `<name> <x.y.z>` to stdout and exit 0.",
    },
    // Errors
    Def {
        id: "errors.exit-code",
        category: Category::Errors,
        title: "Unknown flags fail with a non-zero exit",
        weight: 10,
        why: "Exiting 0 on a typo tells scripts, CI and agents that everything worked.",
        fix: "Reject unknown options and exit non-zero (2 is the convention for usage errors).",
    },
    Def {
        id: "errors.stderr",
        category: Category::Errors,
        title: "Errors go to stderr",
        weight: 5,
        why: "An error on stdout gets swallowed by pipes, or saved into the output file.",
        fix: "Write error messages to stderr, keep stdout for real output.",
    },
    Def {
        id: "errors.message",
        category: Category::Errors,
        title: "Errors say what went wrong",
        weight: 4,
        why: "\"invalid arguments\" makes people guess. Naming the bad flag lets them fix it in seconds.",
        fix: "Name the offending input in the message: `error: unknown option '--colour'`.",
    },
    Def {
        id: "errors.concise",
        category: Category::Errors,
        title: "Errors are short",
        weight: 2,
        why: "Dumping the whole help after an error buries the one line that matters.",
        fix: "Print the error plus one hint (`see 'tool --help'`), not the full help.",
    },
    Def {
        id: "errors.usage-code",
        category: Category::Errors,
        title: "Usage errors exit with code 2",
        weight: 1,
        why: "Exit 2 lets scripts tell \"you called me wrong\" apart from \"the task failed\" (exit 1).",
        fix: "Exit 2 for bad flags and arguments. Most argument parsers have a setting for it.",
    },
    Def {
        id: "errors.suggest",
        category: Category::Errors,
        title: "Typos in subcommands get a suggestion",
        weight: 3,
        why: "`did you mean 'build'?` turns a dead end into a one-key fix.",
        fix: "On an unknown subcommand, suggest the closest match (edit distance) and exit non-zero.",
    },
    Def {
        id: "errors.no-traceback",
        category: Category::Errors,
        title: "No stack traces or crashes",
        weight: 8,
        why: "A stack trace for a user mistake looks like a bug and hides the real message.",
        fix: "Catch expected errors at the top level and print one clear line; keep traces behind --debug or an env var.",
    },
    // Output
    Def {
        id: "output.pipe-color",
        category: Category::Output,
        title: "No escape codes when piped",
        weight: 8,
        why: "Color codes in a pipe end up as `^[[31m` junk in files, logs, grep results and agent context.",
        fix: "Only emit color and other escape codes when the stream is a terminal (isatty).",
    },
    Def {
        id: "output.no-color",
        category: Category::Output,
        title: "Respects NO_COLOR",
        weight: 5,
        why: "NO_COLOR=1 is how people who can't or don't want to see color turn it off everywhere (no-color.org).",
        fix: "If NO_COLOR is set to any non-empty value, don't output color.",
    },
    Def {
        id: "output.term-dumb",
        category: Category::Output,
        title: "Plain text when TERM=dumb",
        weight: 2,
        why: "TERM=dumb means the terminal (an editor's shell, a CI runner) can't show escape codes.",
        fix: "Treat TERM=dumb like a non-terminal: no colors, no cursor movement, no spinners.",
    },
    Def {
        id: "output.closed-pipe",
        category: Category::Output,
        title: "Quiet when the pipe closes early",
        weight: 5,
        why: "`tool | head -1` is everyday use. Crashing or printing \"Broken pipe\" when head exits is noise.",
        fix: "Exit quietly on EPIPE (or let SIGPIPE end the process) instead of reporting it as an error.",
    },
    Def {
        id: "output.stderr-quiet",
        category: Category::Output,
        title: "Nothing on stderr for --help and --version",
        weight: 2,
        why: "Warnings on stderr during --help or --version look like problems and clutter scripts.",
        fix: "Don't print warnings, update notices or deprecation messages for --help and --version.",
    },
    Def {
        id: "output.newline",
        category: Category::Output,
        title: "Output ends with a newline",
        weight: 1,
        why: "Without a final newline the shell prompt lands on the same line, and `$(...)` parsing gets fragile.",
        fix: "End output with \\n.",
    },
    Def {
        id: "output.machine",
        category: Category::Output,
        title: "Offers machine-readable output",
        weight: 1,
        why: "--json (or similar) lets scripts and coding agents use your output without scraping it.",
        fix: "Add --json (or --format json) for commands that print data.",
    },
    // Robustness
    Def {
        id: "robust.startup",
        category: Category::Robustness,
        title: "Starts fast",
        weight: 6,
        why: "Slow startup makes every call, completion and script feel sluggish. Aim for under 150 ms.",
        fix: "Load modules lazily, skip network and update checks for --help/--version, avoid heavy init at import time.",
    },
    Def {
        id: "robust.no-hang",
        category: Category::Robustness,
        title: "Doesn't hang without a terminal",
        weight: 3,
        why: "In CI or an agent's shell there's no one to answer a prompt; a tool that waits stalls the whole job.",
        fix: "If stdin isn't a terminal, never prompt: fail with a message, or take input from flags.",
    },
    Def {
        id: "robust.ctrl-c",
        category: Category::Robustness,
        title: "Ctrl-C works",
        weight: 5,
        why: "If Ctrl-C doesn't work, people reach for kill -9 and lose trust in your tool.",
        fix: "Don't swallow SIGINT: on Ctrl-C, clean up quickly and exit (130 is the convention). REPLs may cancel the line instead, as long as a second Ctrl-C or Ctrl-D exits.",
    },
    Def {
        id: "robust.not-silent",
        category: Category::Robustness,
        title: "Never waits silently",
        weight: 2,
        why: "A blank screen with a blinking cursor gives no clue whether the tool is busy, stuck or waiting for input.",
        fix: "With no arguments in a terminal, print help or a prompt instead of silently reading stdin.",
    },
    Def {
        id: "robust.no-litter",
        category: Category::Robustness,
        title: "--help and --version write no files",
        weight: 3,
        why: "Creating logs, caches or config files just to print help leaves junk in people's folders.",
        fix: "Create files lazily, only when a command actually needs them.",
    },
];

pub fn def(id: &str) -> &'static Def {
    DEFS.iter()
        .find(|d| d.id == id)
        .unwrap_or_else(|| panic!("unknown check {id}"))
}

pub struct CheckResult {
    pub def: &'static Def,
    pub status: Status,
    pub detail: String,
}

pub struct Report {
    pub command: String,
    pub results: Vec<CheckResult>,
    pub duration_ms: u128,
}

fn score_of<'a>(results: impl Iterator<Item = &'a CheckResult>) -> Option<u32> {
    let mut total = 0.0;
    let mut earned = 0.0;
    for r in results.filter(|r| r.status != Status::Skip) {
        total += r.def.weight as f64;
        earned += r.def.weight as f64 * r.status.credit();
    }
    (total > 0.0).then(|| (earned / total * 100.0).round() as u32)
}

impl Report {
    pub fn score(&self) -> u32 {
        score_of(self.results.iter()).unwrap_or(0)
    }

    pub fn category_score(&self, cat: Category) -> Option<u32> {
        score_of(self.results.iter().filter(|r| r.def.category == cat))
    }

    pub fn count(&self, status: Status) -> usize {
        self.results.iter().filter(|r| r.status == status).count()
    }
}

/// `tool --flag` as people would type it.
fn shown(cmd: &str, args: &str) -> String {
    if args.is_empty() {
        format!("`{cmd}`")
    } else {
        format!("`{cmd} {args}`")
    }
}

fn first_line(s: &str) -> String {
    let line = text::strip_ansi(s)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    if line.chars().count() > 70 {
        format!("{}…", line.chars().take(69).collect::<String>())
    } else {
        line
    }
}

fn line_count(s: &str) -> usize {
    s.lines().filter(|l| !l.trim().is_empty()).count()
}

fn ms(d: std::time::Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 10.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}

pub fn evaluate(cmd: &str, p: &Probes, skip: &[String]) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let mut push = |id: &str, status: Status, detail: String| {
        let (status, detail) = if skip.iter().any(|s| s == id) {
            (Status::Skip, "skipped with --skip".to_string())
        } else {
            (status, detail)
        };
        out.push(CheckResult {
            def: def(id),
            status,
            detail,
        });
    };
    use Status::*;

    // help.flag
    let help_out = p.help.combined();
    let any_help_output = !help_out.trim().is_empty();
    // Content checks only make sense when --help actually worked.
    let help_printed = p.help.success() && any_help_output;
    let (st, d) = if p.help.timed_out {
        (
            Fail,
            format!("{} was still running when stopped", shown(cmd, "--help")),
        )
    } else if help_printed {
        (Pass, format!("{} lines of help", line_count(&help_out)))
    } else if p.help.success() {
        (
            Fail,
            format!("{} exited 0 but printed nothing", shown(cmd, "--help")),
        )
    } else if any_help_output {
        (
            Fail,
            format!(
                "{} ended with {}: `{}`",
                shown(cmd, "--help"),
                p.help.status_text(),
                first_line(&p.help.stderr_or_stdout())
            ),
        )
    } else {
        (
            Fail,
            format!(
                "{} ended with {} and printed nothing",
                shown(cmd, "--help"),
                p.help.status_text()
            ),
        )
    };
    push("help.flag", st, d);

    // help.stdout
    let (st, d) = if !help_printed {
        (Skip, "--help didn't work".into())
    } else if p.help.stdout.trim().is_empty() {
        (
            Fail,
            format!(
                "all of it went to stderr; {} shows nothing",
                shown(cmd, "--help | less")
            ),
        )
    } else {
        (Pass, "help is on stdout".into())
    };
    push("help.stdout", st, d);

    // help.short
    let short_out = p.help_short.combined();
    let (st, d) = if p.help_short.success() && line_count(&short_out) >= 2 {
        (Pass, "-h shows help".into())
    } else if p.help_short.timed_out {
        (
            Warn,
            format!("{} was still running when stopped", shown(cmd, "-h")),
        )
    } else if p.help_short.success() {
        (
            Warn,
            format!("{} exits 0 but doesn't print help", shown(cmd, "-h")),
        )
    } else {
        (
            Warn,
            format!(
                "{} ended with {}",
                shown(cmd, "-h"),
                p.help_short.status_text()
            ),
        )
    };
    push("help.short", st, d);

    let info = &p.help_info;
    let (st, d) = match (help_printed, info.has_usage) {
        (false, _) => (Skip, "--help didn't work".into()),
        (true, true) => (Pass, "has a usage line".into()),
        (true, false) => (Fail, "no line starting with `Usage:`".into()),
    };
    push("help.usage", st, d);

    let (st, d) = match (help_printed, info.has_examples) {
        (false, _) => (Skip, "--help didn't work".into()),
        (true, true) => (Pass, "help includes examples".into()),
        (true, false) => (Fail, "no examples in --help".into()),
    };
    push("help.examples", st, d);

    // help.width
    let widest = text::max_width(&help_out);
    let (st, d) = if !help_printed {
        (Skip, "--help didn't work".into())
    } else if widest <= 100 {
        (Pass, format!("widest line is {widest} columns"))
    } else {
        let n = text::lines_wider_than(&help_out, 100);
        let lines = if n == 1 { "line is" } else { "lines are" };
        let st = if widest <= 120 { Warn } else { Fail };
        (
            st,
            format!("{n} {lines} wider than 100 columns (widest {widest})"),
        )
    };
    push("help.width", st, d);

    // help.subcommand
    let (st, d) = match &p.sub_help {
        None => (Skip, "no subcommands found in --help".into()),
        Some((sub, o)) => {
            let args = format!("{sub} --help");
            if o.success() && line_count(&o.combined()) >= 2 {
                (Pass, format!("{} works", shown(cmd, &args)))
            } else if o.timed_out {
                (
                    Fail,
                    format!("{} was still running when stopped", shown(cmd, &args)),
                )
            } else {
                (
                    Fail,
                    format!("{} ended with {}", shown(cmd, &args), o.status_text()),
                )
            }
        }
    };
    push("help.subcommand", st, d);

    let (st, d) = match (help_printed, info.has_url) {
        (false, _) => (Skip, "--help didn't work".into()),
        (true, true) => (Pass, "help includes a link".into()),
        (true, false) => (Warn, "no URL in --help".into()),
    };
    push("help.docs", st, d);

    // help.version
    let v = &p.version;
    let v_out = v.combined();
    let (st, d) = if v.success() && text::has_version_number(&v_out) {
        (Pass, format!("`{}`", first_line(&v_out)))
    } else if v.success() && !v_out.trim().is_empty() {
        (
            Warn,
            format!("no version number in `{}`", first_line(&v_out)),
        )
    } else if v.timed_out {
        (
            Fail,
            format!("{} was still running when stopped", shown(cmd, "--version")),
        )
    } else {
        (
            Fail,
            format!("{} ended with {}", shown(cmd, "--version"), v.status_text()),
        )
    };
    push("help.version", st, d);

    // errors.*
    let bad = &p.bad_flag;
    let bad_out = bad.combined();
    let bad_cmd = shown(cmd, BOGUS_FLAG);
    let bad_failed_properly = bad.ran() && !bad.timed_out && bad.code.is_some_and(|c| c != 0);
    let (st, d) = if bad.timed_out {
        (Fail, format!("{bad_cmd} was still running when stopped"))
    } else if bad.code == Some(0) {
        (Fail, format!("{bad_cmd} exited 0"))
    } else if let Some(sig) = bad.signal.filter(|s| is_crash_signal(*s)) {
        (Fail, format!("{bad_cmd} crashed ({})", signal_name(sig)))
    } else {
        (Pass, format!("{bad_cmd} ended with {}", bad.status_text()))
    };
    push("errors.exit-code", st, d);

    let (st, d) = if !bad_failed_properly || bad_out.trim().is_empty() {
        (Skip, "no error message to check".into())
    } else if bad.stderr.trim().is_empty() {
        (Fail, "the error went to stdout".into())
    } else {
        (Pass, "error is on stderr".into())
    };
    push("errors.stderr", st, d);

    let flag_name = BOGUS_FLAG.trim_start_matches('-');
    let (st, d) = if !bad.ran() || bad.timed_out || bad.code == Some(0) {
        (Skip, "no error to check".into())
    } else if bad_out.trim().is_empty() {
        (Fail, format!("{bad_cmd} failed without printing anything"))
    } else if bad_out.contains(flag_name) {
        (Pass, format!("`{}`", first_line(&bad.stderr_or_stdout())))
    } else {
        (
            Warn,
            format!(
                "message doesn't name the flag: `{}`",
                first_line(&bad.stderr_or_stdout())
            ),
        )
    };
    push("errors.message", st, d);

    let n = line_count(&bad_out);
    let (st, d) = if !bad_failed_properly || n == 0 {
        (Skip, "no error to check".into())
    } else if n <= 25 {
        (Pass, format!("{n} lines"))
    } else {
        (Warn, format!("printed {n} lines for one bad flag"))
    };
    push("errors.concise", st, d);

    let (st, d) = match bad.code {
        Some(2) if bad_failed_properly => (Pass, "exit 2".into()),
        Some(c) if bad_failed_properly => (Warn, format!("exit {c}")),
        _ => (Skip, "no usage error to check".into()),
    };
    push("errors.usage-code", st, d);

    let (st, d) = match &p.typo {
        None => (Skip, "no subcommands to misspell".into()),
        Some((real, typo, o)) => {
            let out = o.combined();
            if o.timed_out {
                (
                    Fail,
                    format!("{} was still running when stopped", shown(cmd, typo)),
                )
            } else if o.code == Some(0) {
                (Fail, format!("{} exited 0", shown(cmd, typo)))
            } else if out.contains(real.as_str()) {
                (Pass, format!("{} suggests `{real}`", shown(cmd, typo)))
            } else {
                (
                    Warn,
                    format!("{} doesn't suggest `{real}`", shown(cmd, typo)),
                )
            }
        }
    };
    push("errors.suggest", st, d);

    let mut crash: Option<String> = None;
    for (args, o) in p.piped().into_iter().chain(p.in_terminal()) {
        if !o.ran() {
            continue;
        }
        if let Some(what) = text::traceback(&o.combined()) {
            crash = Some(format!("{} printed {what}", shown(cmd, &args)));
            break;
        }
        if let Some(sig) = o.signal.filter(|s| is_crash_signal(*s)) {
            crash = Some(format!(
                "{} crashed ({})",
                shown(cmd, &args),
                signal_name(sig)
            ));
            break;
        }
    }
    let (st, d) = match crash {
        Some(c) => (Fail, c),
        None => (Pass, "no stack traces in any run".into()),
    };
    push("errors.no-traceback", st, d);

    // output.pipe-color
    let mut piped_escape = None;
    for (args, o) in p.piped() {
        if o.ran() && text::escapes(&o.combined()).any {
            piped_escape = Some(args);
            break;
        }
    }
    let (st, d) = match piped_escape {
        Some(args) => (
            Fail,
            format!("{} has escape codes in a pipe", shown(cmd, &args)),
        ),
        None => (Pass, "plain text in every piped run".into()),
    };
    push("output.pipe-color", st, d);

    // output.no-color
    let tty_ok = !p.help_pty.unsupported;
    let colored = [&p.help_pty, &p.bad_flag_pty]
        .iter()
        .any(|o| o.ran() && text::escapes(&o.stdout).color);
    let still_colored = [
        (&p.help_pty_no_color, "--help"),
        (&p.bad_flag_pty_no_color, BOGUS_FLAG),
    ]
    .into_iter()
    .find(|(o, _)| o.ran() && text::escapes(&o.stdout).color);
    let (st, d) = if !tty_ok {
        (Skip, "needs a terminal (not supported on this OS)".into())
    } else if let Some((_, args)) = still_colored {
        (Fail, format!("NO_COLOR=1 {cmd} {args} still prints color"))
    } else if !colored {
        (Skip, "doesn't use color".into())
    } else {
        (Pass, "color turns off with NO_COLOR=1".into())
    };
    push("output.no-color", st, d);

    let any_escape = [&p.help_pty, &p.bad_flag_pty]
        .iter()
        .any(|o| o.ran() && text::escapes(&o.stdout).any);
    let dumb = text::escapes(&p.help_pty_dumb.stdout);
    let (st, d) = if !tty_ok {
        (Skip, "needs a terminal (not supported on this OS)".into())
    } else if !p.help_pty_dumb.ran() {
        (Skip, "could not run in a terminal".into())
    } else if dumb.color {
        (Fail, format!("TERM=dumb {cmd} --help still prints color"))
    } else if dumb.any {
        (
            Warn,
            format!("TERM=dumb {cmd} --help still prints escape codes"),
        )
    } else if !any_escape {
        (Skip, "doesn't use escape codes".into())
    } else {
        (Pass, "plain text with TERM=dumb".into())
    };
    push("output.term-dumb", st, d);

    // output.closed-pipe
    let c = &p.closed_stdout;
    let complaint = {
        let lower = c.stderr.to_lowercase();
        lower.contains("broken pipe") || lower.contains("epipe")
    };
    let (st, d) = if c.unsupported {
        (Skip, "not supported on this OS".into())
    } else if !c.ran() {
        (Skip, "could not run".into())
    } else if c.timed_out {
        (
            Fail,
            format!(
                "{} hung when its output pipe was closed",
                shown(cmd, "--help")
            ),
        )
    } else if let Some(what) = text::traceback(&c.stderr) {
        (Fail, format!("printed {what} when the pipe closed"))
    } else if let Some(sig) = c.signal.filter(|s| is_crash_signal(*s)) {
        (
            Fail,
            format!("crashed ({}) when the pipe closed", signal_name(sig)),
        )
    } else if complaint {
        (Fail, format!("printed `{}`", first_line(&c.stderr)))
    } else {
        (Pass, "exits quietly when the reader goes away".into())
    };
    push("output.closed-pipe", st, d);

    let noisy: Vec<&str> = [(&p.help, "--help"), (&p.version, "--version")]
        .into_iter()
        .filter(|(o, _)| o.success() && !o.stderr.trim().is_empty())
        .map(|(_, a)| a)
        .collect();
    let (st, d) = if !p.help.success() && !p.version.success() {
        (Skip, "neither --help nor --version succeeded".into())
    } else if noisy.is_empty() {
        (Pass, "stderr stays empty".into())
    } else {
        let o = if noisy[0] == "--help" {
            &p.help
        } else {
            &p.version
        };
        (
            Warn,
            format!(
                "{} wrote to stderr: `{}`",
                noisy.join(" and "),
                first_line(&o.stderr)
            ),
        )
    };
    push("output.stderr-quiet", st, d);

    let (st, d) = if !v.success() || v.stdout.is_empty() {
        (Skip, "no --version output to check".into())
    } else if v.stdout.ends_with('\n') {
        (Pass, "--version ends with a newline".into())
    } else {
        (Warn, "--version output has no final newline".into())
    };
    push("output.newline", st, d);

    let (st, d) = match (help_printed, info.machine_output) {
        (false, _) => (Skip, "--help didn't work".into()),
        (true, true) => (Pass, "help mentions --json or a format option".into()),
        (true, false) => (Warn, "no --json or --format option in help".into()),
    };
    push("output.machine", st, d);

    // robust.startup
    let (st, d) = if p.timing.is_empty() {
        (
            Skip,
            "nothing to time (--help and --version both failed)".into(),
        )
    } else {
        let mut t = p.timing.clone();
        t.sort();
        let median = t[t.len() / 2];
        let detail = format!("median {} for {}", ms(median), shown(cmd, p.timing_args));
        let st = if median.as_millis() <= 150 {
            Pass
        } else if median.as_millis() <= 500 {
            Warn
        } else {
            Fail
        };
        (st, detail)
    };
    push("robust.startup", st, d);

    // robust.no-hang
    let (st, d) = match &p.bare {
        None => (Skip, p.bare_skipped_reason.clone().unwrap_or_default()),
        Some(o) if o.timed_out => (
            Warn,
            format!(
                "{} kept running with no terminal (fine for servers; otherwise exit or print help)",
                shown(cmd, "")
            ),
        ),
        Some(o) => (
            Pass,
            format!("{} returns ({})", shown(cmd, ""), o.status_text()),
        ),
    };
    push("robust.no-hang", st, d);

    // robust.ctrl-c and robust.not-silent
    let (ctrl, silent) = match &p.bare_pty {
        None => {
            let why = p.bare_skipped_reason.clone().unwrap_or_default();
            ((Skip, why.clone()), (Skip, why))
        }
        Some(o) if o.unsupported => {
            let why = "needs a terminal (not supported on this OS)".to_string();
            ((Skip, why.clone()), (Skip, why))
        }
        Some(o) if !o.ctrl_c_sent => {
            let why = format!("{} exits on its own", shown(cmd, ""));
            ((Skip, why.clone()), (Pass, why))
        }
        Some(o) => {
            let ctrl = match (o.exit_stage, o.ctrl_c_exit, o.ctrl_c_reacted) {
                (Some(1), Some(t), _) => (Pass, format!("stopped {} after Ctrl-C", ms(t))),
                (Some(2), _, _) => (
                    Pass,
                    "Ctrl-C cancels, a second Ctrl-C exits (REPL-style)".into(),
                ),
                (Some(3), _, true) => (
                    Pass,
                    "Ctrl-C cancels input, Ctrl-D exits (REPL-style)".into(),
                ),
                (Some(_), _, _) => (Warn, "ignored Ctrl-C; only Ctrl-D stopped it".into()),
                (None, _, true) => (
                    Warn,
                    "reacts to Ctrl-C but keeps running, even after Ctrl-D".into(),
                ),
                (None, _, false) => (Fail, "ignored Ctrl-C twice and Ctrl-D".into()),
            };
            let shown_text = text::strip_ansi(&o.before_ctrl_c);
            let silent = if shown_text.trim().is_empty() {
                (
                    Warn,
                    format!("{} waits with a blank screen", shown(cmd, "")),
                )
            } else {
                (Pass, format!("shows `{}`", first_line(&shown_text)))
            };
            (ctrl, silent)
        }
    };
    push("robust.ctrl-c", ctrl.0, ctrl.1);
    push("robust.not-silent", silent.0, silent.1);

    let (st, d) = if p.litter.is_empty() {
        (Pass, "no new files".into())
    } else {
        let mut list = p
            .litter
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if p.litter.len() > 3 {
            list.push_str(&format!(" and {} more", p.litter.len() - 3));
        }
        (Fail, format!("created {list}"))
    };
    push("robust.no-litter", st, d);

    out
}

trait StderrFirst {
    fn stderr_or_stdout(&self) -> String;
}

impl StderrFirst for Outcome {
    fn stderr_or_stdout(&self) -> String {
        if self.stderr.trim().is_empty() {
            self.stdout.clone()
        } else {
            self.stderr.clone()
        }
    }
}

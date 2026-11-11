mod checks;
mod probe;
mod report;
mod runner;
mod text;

use clap::{Parser, ValueEnum};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const EXAMPLES: &str = "\
Examples:
  mannerly rg                          Check ripgrep
  mannerly ./target/debug/mytool       Check your own build
  mannerly cargo clippy                Check a subcommand
  mannerly --min-score 90 ./mytool     Fail CI below 90
  mannerly --json -- npx mytool        Use -- when the command has flags

Docs and issues: https://github.com/Baimmmss/mannerly";

/// Lighthouse for command-line tools.
///
/// Runs a command the way people, scripts and coding agents do (with --help,
/// a bad flag, in a pipe, in a terminal, with NO_COLOR, with Ctrl-C) and scores
/// how well it behaves.
#[derive(Parser, Debug)]
#[command(name = "mannerly", version, after_help = EXAMPLES, max_term_width = 100)]
struct Cli {
    /// The command to check, plus any arguments that always come first
    #[arg(
        value_name = "COMMAND",
        trailing_var_arg = true,
        num_args = 1..,
        required_unless_present_any = ["list_checks", "explain"]
    )]
    command: Vec<String>,

    /// Print the report as JSON
    #[arg(long, conflicts_with = "markdown")]
    json: bool,

    /// Print the report as Markdown, for PR comments and CI job summaries
    #[arg(long)]
    markdown: bool,

    /// Also write an SVG score badge to this file
    #[arg(long, value_name = "FILE")]
    badge: Option<PathBuf>,

    /// Exit with code 1 if the score is below this (for CI)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(0..=100))]
    min_score: Option<u32>,

    /// Show every check, including passes and skips
    #[arg(short, long)]
    verbose: bool,

    /// Only print the score
    #[arg(short, long, conflicts_with_all = ["json", "markdown", "verbose"])]
    quiet: bool,

    /// Skip a check by id (repeat, or separate with commas)
    #[arg(long, value_name = "ID", value_delimiter = ',')]
    skip: Vec<String>,

    /// Seconds to wait for each run of the command
    #[arg(long, value_name = "SECS", default_value_t = 10.0)]
    timeout: f64,

    /// Never run the command with no arguments
    #[arg(long)]
    no_bare: bool,

    /// When to use color
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    color: ColorChoice,

    /// List every check and exit
    #[arg(long)]
    list_checks: bool,

    /// Explain one check and exit
    #[arg(long, value_name = "ID")]
    explain: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

fn env_set(key: &str) -> bool {
    std::env::var_os(key).is_some_and(|v| !v.is_empty())
}

fn use_color(choice: ColorChoice, is_tty: bool) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            if env_set("NO_COLOR") {
                false
            } else if env_set("CLICOLOR_FORCE") || env_set("FORCE_COLOR") {
                true
            } else {
                is_tty && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
            }
        }
    }
}

fn terminal_width() -> Option<usize> {
    if let Some(cols) = std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()) {
        return Some(cols);
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        for fd in [std::io::stdout().as_raw_fd(), std::io::stderr().as_raw_fd()] {
            // SAFETY: TIOCGWINSZ fills a winsize; failure leaves it zeroed.
            if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as _, &mut size) } == 0 && size.ws_col > 0
            {
                return Some(size.ws_col as usize);
            }
        }
    }
    None
}

fn fail(msg: &str, hint: Option<&str>) -> ExitCode {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "mannerly: {msg}");
    if let Some(h) = hint {
        let _ = writeln!(err, "{h}");
    }
    ExitCode::FAILURE
}

/// Shows what's happening on stderr while the checks run, if stderr is a terminal.
struct Progress {
    active: bool,
}

impl Progress {
    fn start(what: &str, active: bool) -> Self {
        if active {
            eprint!("  checking {what} …");
            let _ = std::io::stderr().flush();
        }
        Progress { active }
    }
    fn done(self) {
        if self.active {
            eprint!("\r\x1b[2K");
            let _ = std::io::stderr().flush();
        }
    }
}

fn main() -> ExitCode {
    // Writing to a closed pipe (`mannerly x | head -1`) should end quietly.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    let stdout_tty = std::io::stdout().is_terminal();
    let style = report::Style {
        color: use_color(cli.color, stdout_tty),
        width: if stdout_tty {
            terminal_width().unwrap_or(80)
        } else {
            100
        },
    };

    if cli.list_checks {
        print!("{}", report::list_checks(&style));
        return ExitCode::SUCCESS;
    }
    if let Some(id) = &cli.explain {
        return match report::explain(id, &style) {
            Some(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            None => fail(
                &format!("no check called '{id}'"),
                Some("See every check with: mannerly --list-checks"),
            ),
        };
    }
    for id in &cli.skip {
        if !checks::DEFS.iter().any(|d| d.id == id) {
            return fail(
                &format!("--skip: no check called '{id}'"),
                Some("See every check with: mannerly --list-checks"),
            );
        }
    }
    if !(cli.timeout.is_finite() && cli.timeout > 0.0 && cli.timeout <= 600.0) {
        return fail("--timeout must be between 0 and 600 seconds", None);
    }

    // Runs happen in a temp folder, so a relative path must be resolved first.
    let mut program = cli.command[0].clone();
    if program.contains(std::path::MAIN_SEPARATOR) || program.contains('/') {
        match std::fs::canonicalize(&program) {
            Ok(p) => program = p.to_string_lossy().into_owned(),
            Err(e) => return fail(&format!("{program}: {e}"), None),
        }
    }
    let target = probe::Target {
        program,
        prefix: cli.command[1..].to_vec(),
    };
    let opts = runner::Options {
        timeout: Duration::from_secs_f64(cli.timeout),
        bare: !cli.no_bare,
    };
    let (clean, work) = match (tempfile::tempdir(), tempfile::tempdir()) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            return fail(&format!("could not create a temp folder: {e}"), None);
        }
    };

    let started = Instant::now();
    let show_progress = std::io::stderr().is_terminal() && !cli.quiet;
    let shown_command = cli.command.join(" ");
    let progress = Progress::start(&shown_command, show_progress);
    let help = match runner::first_contact(&target, &opts, work.path()) {
        Ok(h) => h,
        Err(e) => {
            progress.done();
            let hint = (!target.program.contains('/')).then_some(
                "Is it on your PATH? To check a local build, pass a path like ./target/debug/mytool",
            );
            return fail(&e, hint);
        }
    };
    let probes = runner::collect(&target, &opts, help, clean.path(), work.path());
    progress.done();
    if env_set("MANNERLY_DUMP") {
        probes.dump();
    }

    let command = shown_command;
    let report = checks::Report {
        results: checks::evaluate(&command, &probes, &cli.skip),
        command,
        duration_ms: started.elapsed().as_millis(),
    };
    let score = report.score();

    if let Some(path) = &cli.badge
        && let Err(e) = std::fs::write(path, report::badge(score))
    {
        return fail(&format!("could not write {}: {e}", path.display()), None);
    }

    let out = if cli.json {
        report::json(&report)
    } else if cli.markdown {
        report::markdown(&report)
    } else if cli.quiet {
        format!("{score}\n")
    } else {
        report::terminal(&report, &style, cli.verbose)
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(out.as_bytes())
        .and_then(|_| stdout.flush())
        .is_err()
    {
        return ExitCode::FAILURE;
    }

    match cli.min_score {
        Some(min) if score < min => {
            drop(stdout);
            eprintln!("mannerly: score {score} is below --min-score {min}");
            ExitCode::FAILURE
        }
        _ => ExitCode::SUCCESS,
    }
}

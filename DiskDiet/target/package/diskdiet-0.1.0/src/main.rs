mod caches;
mod delete;
mod output;
mod rules;
mod scan;
mod tui;
mod units;

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;

use scan::{Entry, Options, Progress, Source};

/// Find and delete the gigabytes your dev tools leave behind.
///
/// diskdiet scans for node_modules, Rust/Gradle/Maven targets, Python
/// virtualenvs, Xcode DerivedData, package-manager caches and dozens of other kinds
/// of rebuildable clutter, then lets you pick what to remove.
#[derive(Parser, Debug)]
#[command(name = "diskdiet", version, about, long_about, after_help = EXAMPLES)]
struct Cli {
    /// Folders to scan [default: your home folder, plus global caches]
    paths: Vec<PathBuf>,

    /// Print a table instead of opening the interactive UI
    #[arg(short, long)]
    list: bool,

    /// Print results as JSON (for scripts)
    #[arg(long, conflicts_with = "list")]
    json: bool,

    /// Delete everything that matches, without the interactive UI
    #[arg(long, conflicts_with = "json")]
    delete: bool,

    /// Don't ask for confirmation with --delete
    #[arg(short, long, requires = "delete")]
    yes: bool,

    /// Only show projects untouched for at least this long (e.g. 30d, 2w, 6m, 1y)
    #[arg(short = 'o', long, value_name = "AGE", value_parser = units::parse_duration)]
    older_than: Option<Duration>,

    /// Only show folders at least this big (e.g. 100MB, 1GB)
    #[arg(short = 'm', long, value_name = "SIZE", value_parser = units::parse_size)]
    min_size: Option<u64>,

    /// Only these ecosystems, comma separated (e.g. node,rust,python)
    #[arg(long, value_delimiter = ',', value_name = "LIST")]
    only: Vec<String>,

    /// Skip these ecosystems, comma separated
    #[arg(long, value_delimiter = ',', value_name = "LIST")]
    exclude: Vec<String>,

    /// Also check global caches when scanning specific folders
    #[arg(long, conflicts_with_all = ["no_caches", "caches_only"])]
    caches: bool,

    /// Never check global caches
    #[arg(long, conflicts_with = "caches_only")]
    no_caches: bool,

    /// Only check global caches (npm, Cargo, Xcode, pip, Gradle, …)
    #[arg(long)]
    caches_only: bool,

    /// List every ecosystem diskdiet knows about and exit
    #[arg(long)]
    ecosystems: bool,
}

const EXAMPLES: &str = "\
Examples:
  diskdiet                         interactive cleanup of your home folder
  diskdiet ~/code -o 3m            projects untouched for 3+ months
  diskdiet --caches-only --list    how big are my package-manager caches?
  diskdiet ~/code --only node -o 30d --delete
  diskdiet --json | jq '.total_bytes'";

/// Restrictions given on the command line.
#[derive(Debug, Default)]
pub struct Filter {
    pub older_than: Option<Duration>,
    pub min_size: u64,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
}

impl Filter {
    pub fn keep(&self, e: &Entry) -> bool {
        e.bytes >= self.min_size
            && (self.only.is_empty() || self.only.iter().any(|o| o == e.ecosystem))
            && !self.exclude.iter().any(|x| x == e.ecosystem)
            && self.older_than.is_none_or(|min| {
                e.source == Source::Project && units::age(e.last_modified).is_some_and(|a| a >= min)
            })
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("diskdiet: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    if cli.ecosystems {
        println!("{}", rules::ecosystems().join("\n"));
        return Ok(ExitCode::SUCCESS);
    }

    let known = rules::ecosystems();
    let norm = |v: Vec<String>| -> Result<Vec<String>, String> {
        v.into_iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .map(|s| match s.as_str() {
                "js" | "javascript" | "ts" | "npm" => Ok("node".to_string()),
                "py" => Ok("python".to_string()),
                "cargo" => Ok("rust".to_string()),
                "c" | "cpp" | "c++" => Ok("c/c++".to_string()),
                "dotnet" => Ok(".net".to_string()),
                _ if known.contains(&s.as_str()) => Ok(s),
                _ => Err(format!(
                    "unknown ecosystem `{s}`. Known: {}",
                    known.join(", ")
                )),
            })
            .collect()
    };
    let filter = Filter {
        older_than: cli.older_than,
        min_size: cli.min_size.unwrap_or(0),
        only: norm(cli.only)?,
        exclude: norm(cli.exclude)?,
    };

    let default_home = cli.paths.is_empty();
    let roots = if default_home {
        vec![caches::home_dir().ok_or("could not find your home folder; pass a path")?]
    } else {
        for p in &cli.paths {
            if !p.is_dir() {
                return Err(format!("`{}` is not a folder", p.display()));
            }
        }
        cli.paths
    };
    let opts = Options {
        roots,
        projects: !cli.caches_only,
        caches: cli.caches_only || cli.caches || (default_home && !cli.no_caches),
    };

    let interactive = io::stdout().is_terminal() && io::stdin().is_terminal();
    if cli.json {
        let entries = collect(&opts, &filter, false);
        println!("{}", output::json(&entries));
    } else if cli.delete {
        return delete_all(&opts, &filter, cli.yes, interactive);
    } else if cli.list || !interactive {
        let entries = collect(&opts, &filter, io::stderr().is_terminal());
        output::table(&entries, io::stdout().is_terminal());
    } else {
        let freed = tui::run(opts, filter).map_err(|e| format!("terminal error: {e}"))?;
        if freed > 0 {
            println!(
                "\n  \x1b[1;32m✨ diskdiet freed {}\x1b[0m — your disk thanks you.\n",
                units::bytes(freed)
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Runs a scan to completion, optionally drawing a progress line on stderr.
fn collect(opts: &Options, filter: &Filter, show_progress: bool) -> Vec<Entry> {
    let (tx, rx) = mpsc::channel();
    let progress = Arc::new(Progress::default());
    let started = Instant::now();
    let worker = {
        let progress = progress.clone();
        let opts = Options {
            roots: opts.roots.clone(),
            projects: opts.projects,
            caches: opts.caches,
        };
        thread::spawn(move || scan::run(&opts, &progress, &tx))
    };

    let mut entries = Vec::new();
    let mut total = 0;
    let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut tick = 0;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(e) => {
                if filter.keep(&e) {
                    total += e.bytes;
                    entries.push(e);
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if show_progress {
            tick += 1;
            eprint!(
                "\r\x1b[2K{} scanning… {} folders · found {}",
                spinner[tick % spinner.len()],
                units::count(progress.dirs.load(Ordering::Relaxed)),
                units::bytes(total)
            );
            let _ = io::stderr().flush();
        }
    }
    let _ = worker.join();
    if show_progress {
        eprintln!(
            "\r\x1b[2KScanned {} folders in {:.1}s",
            units::count(progress.dirs.load(Ordering::Relaxed)),
            started.elapsed().as_secs_f64()
        );
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    entries
}

fn delete_all(
    opts: &Options,
    filter: &Filter,
    yes: bool,
    interactive: bool,
) -> Result<ExitCode, String> {
    let entries = collect(opts, filter, io::stderr().is_terminal());
    if entries.is_empty() {
        println!("Nothing to delete.");
        return Ok(ExitCode::SUCCESS);
    }
    let color = io::stdout().is_terminal();
    output::table(&entries, color);
    let total: u64 = entries.iter().map(|e| e.bytes).sum();

    if !yes {
        if !interactive {
            return Err("refusing to delete without a terminal; pass --yes to confirm".into());
        }
        print!(
            "\nPermanently delete these {} folders ({})? [y/N] ",
            entries.len(),
            units::bytes(total)
        );
        let _ = io::stdout().flush();
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|e| e.to_string())?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Cancelled, nothing was deleted.");
            return Ok(ExitCode::SUCCESS);
        }
    }

    let mut freed = 0;
    let mut failed = 0;
    for e in &entries {
        match delete::remove(&e.path) {
            Ok(()) => freed += e.bytes,
            Err(err) => {
                failed += 1;
                eprintln!("  could not delete {}: {err}", output::tilde(&e.path));
            }
        }
    }
    println!(
        "\n✨ Freed {} ({} folders)",
        units::bytes(freed),
        entries.len() - failed
    );
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

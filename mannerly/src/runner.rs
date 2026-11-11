//! Decides which runs to make and makes them.

use crate::probe::{self, Mode, Outcome, Spec, Target};
use crate::text::{self, Help};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The flag we pass to see how the tool handles an unknown option.
pub const BOGUS_FLAG: &str = "--mannerly-unknown-flag";

/// Programs that do something drastic when run with no arguments.
const NO_BARE_RUN: &[&str] = &[
    "reboot",
    "shutdown",
    "halt",
    "poweroff",
    "init",
    "telinit",
    "systemctl",
    "launchctl",
    "kill",
    "killall",
    "pkill",
    "sudo",
    "doas",
    "su",
    "login",
    "logout",
    "exit",
    "yes",
    "top",
    "htop",
    "vim",
    "vi",
    "nvim",
    "emacs",
    "nano",
    "less",
    "more",
    "man",
    "watch",
    "sleep",
];

pub struct Options {
    pub timeout: Duration,
    pub bare: bool,
}

/// Everything we observed about the tool.
pub struct Probes {
    pub help: Outcome,
    pub help_short: Outcome,
    pub version: Outcome,
    pub bad_flag: Outcome,
    pub bare: Option<Outcome>,
    pub bare_skipped_reason: Option<String>,
    pub help_pty: Outcome,
    pub help_pty_no_color: Outcome,
    pub help_pty_dumb: Outcome,
    pub bad_flag_pty: Outcome,
    pub bad_flag_pty_no_color: Outcome,
    pub closed_stdout: Outcome,
    pub bare_pty: Option<Outcome>,
    pub sub_help: Option<(String, Outcome)>,
    pub typo: Option<(String, String, Outcome)>,
    pub timing: Vec<Duration>,
    pub timing_args: &'static str,
    pub litter: Vec<String>,
    pub help_info: Help,
}

impl Probes {
    /// Prints every run, for debugging mannerly itself (MANNERLY_DUMP=1).
    pub fn dump(&self) {
        let mut all = self.piped();
        all.extend(self.in_terminal());
        all.push(("--help (closed stdout)".into(), &self.closed_stdout));
        for (args, o) in all {
            eprintln!(
                "== [{args}] {} in {:?} ctrl_c_sent={} ctrl_c_exit={:?}\n-- stdout: {:?}\n-- stderr: {:?}",
                o.status_text(),
                o.duration,
                o.ctrl_c_sent,
                o.ctrl_c_exit,
                o.stdout,
                o.stderr
            );
        }
        eprintln!(
            "== timing {:?}\n== litter {:?}\n== subcommands {:?}",
            self.timing, self.litter, self.help_info.subcommands
        );
    }

    /// Every run made with ordinary pipes, labeled with the arguments used.
    pub fn piped(&self) -> Vec<(String, &Outcome)> {
        let mut v = vec![
            ("--help".to_string(), &self.help),
            ("-h".to_string(), &self.help_short),
            ("--version".to_string(), &self.version),
            (BOGUS_FLAG.to_string(), &self.bad_flag),
        ];
        if let Some(o) = &self.bare {
            v.push((String::new(), o));
        }
        if let Some((sub, o)) = &self.sub_help {
            v.push((format!("{sub} --help"), o));
        }
        if let Some((_, typo, o)) = &self.typo {
            v.push((typo.clone(), o));
        }
        v
    }

    /// Every run made in a terminal.
    pub fn in_terminal(&self) -> Vec<(String, &Outcome)> {
        let mut v = vec![
            ("--help".to_string(), &self.help_pty),
            ("--help".to_string(), &self.help_pty_no_color),
            ("--help".to_string(), &self.help_pty_dumb),
            (BOGUS_FLAG.to_string(), &self.bad_flag_pty),
            (BOGUS_FLAG.to_string(), &self.bad_flag_pty_no_color),
        ];
        if let Some(o) = &self.bare_pty {
            v.push((String::new(), o));
        }
        v
    }
}

fn home_entries() -> BTreeSet<String> {
    let Some(home) = std::env::var_os("HOME") else {
        return BTreeSet::new();
    };
    std::fs::read_dir(home)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn dir_entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn bare_skip_reason(target: &Target, opts: &Options) -> Option<String> {
    if !opts.bare {
        return Some("skipped with --no-bare".into());
    }
    let name = Path::new(&target.program)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if target.prefix.is_empty() && NO_BARE_RUN.contains(&name.as_str()) {
        return Some(format!(
            "not run: `{name}` with no arguments is risky or interactive"
        ));
    }
    None
}

/// Runs the first probe on its own so a missing command fails fast.
pub fn first_contact(target: &Target, opts: &Options, cwd: &Path) -> Result<Outcome, String> {
    let help = probe::run(
        target,
        &Spec::new(&["--help"], Mode::Pipe, opts.timeout, cwd.to_path_buf()),
    );
    match &help.spawn_error {
        Some(e) => Err(e.clone()),
        None => Ok(help),
    }
}

pub fn collect(
    target: &Target,
    opts: &Options,
    help: Outcome,
    clean: &Path,
    work: &Path,
) -> Probes {
    let t = opts.timeout;
    let home_before = home_entries();
    let pipe = |args: &[&str], dir: &Path| {
        probe::run(target, &Spec::new(args, Mode::Pipe, t, dir.to_path_buf()))
    };

    // These run one at a time in an empty folder, so timings are clean and any
    // files the tool writes just to print help or a version are easy to spot.
    let version = pipe(&["--version"], clean);
    let help_short = pipe(&["-h"], clean);
    let timing_args: &'static str = if version.success() {
        "--version"
    } else {
        "--help"
    };
    let mut timing = Vec::new();
    if version.success() || help.success() {
        let _warmup = pipe(&[timing_args], clean);
        for _ in 0..7 {
            let o = pipe(&[timing_args], clean);
            if o.success() {
                timing.push(o.duration);
            }
        }
    }
    let mut litter: Vec<String> = dir_entries(clean)
        .into_iter()
        .map(|f| format!("./{f}"))
        .collect();
    let home_after = home_entries();
    litter.extend(
        home_after
            .difference(&home_before)
            .map(|f| format!("~/{f}")),
    );

    // Slow tools get proportionally more time, so they aren't marked as hanging.
    let median = {
        let mut t = timing.clone();
        t.sort();
        t.get(t.len() / 2).copied().unwrap_or(help.duration)
    };
    let t = t.max(median * 10);
    let ctrl_c_after = Duration::from_secs(3).max(median * 4).min(t);
    let pipe = |args: &[&str], dir: &Path| {
        probe::run(target, &Spec::new(args, Mode::Pipe, t, dir.to_path_buf()))
    };

    let help_info = text::parse_help(&help.combined());
    let sub = help_info
        .subcommands
        .iter()
        .find(|s| s.as_str() != "help")
        .cloned();
    let typo_source = help_info
        .subcommands
        .iter()
        .filter(|s| s.as_str() != "help" && s.len() >= 4)
        .max_by_key(|s| s.len())
        .cloned();
    let bare_skipped_reason = bare_skip_reason(target, opts);
    let run_bare = bare_skipped_reason.is_none();

    let pty = |args: &[&str], cols: u16| {
        Spec::new(
            args,
            Mode::Pty {
                cols,
                rows: 30,
                ctrl_c_after: None,
            },
            t,
            work.to_path_buf(),
        )
    };
    let run = |spec: Spec| probe::run(target, &spec);
    let work_buf: PathBuf = work.to_path_buf();

    std::thread::scope(|s| {
        let bad_flag = s.spawn(|| pipe(&[BOGUS_FLAG], work));
        let bare = s.spawn(|| run_bare.then(|| pipe(&[], work)));
        let help_pty = s.spawn(|| run(pty(&["--help"], 100)));
        let help_pty_no_color = s.spawn(|| run(pty(&["--help"], 100).env("NO_COLOR", Some("1"))));
        let help_pty_dumb = s.spawn(|| run(pty(&["--help"], 100).env("TERM", Some("dumb"))));
        let bad_flag_pty = s.spawn(|| run(pty(&[BOGUS_FLAG], 100)));
        let bad_flag_pty_no_color =
            s.spawn(|| run(pty(&[BOGUS_FLAG], 100).env("NO_COLOR", Some("1"))));
        let closed_stdout = s.spawn(|| {
            run(Spec::new(
                &["--help"],
                Mode::ClosedStdout,
                t,
                work_buf.clone(),
            ))
        });
        let bare_pty = s.spawn(|| {
            run_bare.then(|| {
                run(Spec::new(
                    &[],
                    Mode::Pty {
                        cols: 100,
                        rows: 30,
                        ctrl_c_after: Some(ctrl_c_after),
                    },
                    t + Duration::from_secs(4),
                    work_buf.clone(),
                ))
            })
        });
        let sub_help = s.spawn(|| {
            sub.map(|name| {
                let o = pipe(&[name.as_str(), "--help"], work);
                (name, o)
            })
        });
        let typo = s.spawn(|| {
            typo_source.and_then(|name| {
                let typo = text::typo_of(&name, &help_info.subcommands)?;
                let o = pipe(&[typo.as_str()], work);
                Some((name, typo, o))
            })
        });

        let j = |h: std::thread::ScopedJoinHandle<'_, Outcome>| h.join().unwrap_or_default();
        Probes {
            bad_flag: j(bad_flag),
            bare: bare.join().unwrap_or_default(),
            help_pty: j(help_pty),
            help_pty_no_color: j(help_pty_no_color),
            help_pty_dumb: j(help_pty_dumb),
            bad_flag_pty: j(bad_flag_pty),
            bad_flag_pty_no_color: j(bad_flag_pty_no_color),
            closed_stdout: j(closed_stdout),
            bare_pty: bare_pty.join().unwrap_or_default(),
            sub_help: sub_help.join().unwrap_or_default(),
            typo: typo.join().unwrap_or_default(),
            help,
            help_short,
            version,
            bare_skipped_reason,
            timing,
            timing_args,
            litter,
            help_info: help_info.clone(),
        }
    })
}

//! Runs the command under test in controlled conditions and records what it did.
//!
//! Three ways to run it:
//! - `Pipe`: stdin is `/dev/null`, stdout and stderr are pipes (how scripts and CI see a tool).
//! - `ClosedStdout`: stdout is a pipe whose reader is already gone (like `tool | head -0`).
//! - `Pty`: the tool gets a real pseudo-terminal, as if a person were typing.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Most output we keep from one stream. More is read and thrown away.
const MAX_CAPTURE: usize = 1 << 20;

/// The command being checked: a program plus any arguments that always come first
/// (so `mannerly cargo clippy` checks `cargo clippy --help`, `cargo clippy --version`, ...).
#[derive(Clone, Debug)]
pub struct Target {
    pub program: String,
    pub prefix: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Mode {
    Pipe,
    ClosedStdout,
    Pty {
        cols: u16,
        rows: u16,
        /// If the tool is still running after this long, type Ctrl-C into the terminal.
        ctrl_c_after: Option<Duration>,
    },
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub args: Vec<String>,
    pub mode: Mode,
    /// Variables to set (`Some`) or remove (`None`).
    pub env: Vec<(&'static str, Option<&'static str>)>,
    pub timeout: Duration,
    pub cwd: PathBuf,
}

impl Spec {
    pub fn new(args: &[&str], mode: Mode, timeout: Duration, cwd: PathBuf) -> Self {
        Spec {
            args: args.iter().map(|s| s.to_string()).collect(),
            mode,
            env: Vec::new(),
            timeout,
            cwd,
        }
    }

    pub fn env(mut self, key: &'static str, value: Option<&'static str>) -> Self {
        self.env.push((key, value));
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// For PTY runs this is everything the terminal showed (stdout and stderr together).
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub duration: Duration,
    /// Set when the program could not be started at all.
    pub spawn_error: Option<String>,
    /// Set when this kind of run is not possible on this OS.
    pub unsupported: bool,
    pub ctrl_c_sent: bool,
    /// Time from the last key we sent to exit, if it exited.
    pub ctrl_c_exit: Option<Duration>,
    /// Which key ended it: 1 = first Ctrl-C, 2 = second Ctrl-C, 3 = Ctrl-D.
    pub exit_stage: Option<u8>,
    /// The screen changed after the first Ctrl-C (a REPL printing a new prompt, say).
    pub ctrl_c_reacted: bool,
    /// What the terminal showed before we pressed anything.
    pub before_ctrl_c: String,
}

impl Outcome {
    pub fn ran(&self) -> bool {
        self.spawn_error.is_none() && !self.unsupported
    }

    pub fn success(&self) -> bool {
        self.ran() && !self.timed_out && self.code == Some(0)
    }

    pub fn combined(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }

    /// Exit code, or a description of how it ended, for messages.
    pub fn status_text(&self) -> String {
        if self.timed_out {
            "still running when stopped".into()
        } else if let Some(code) = self.code {
            format!("exit {code}")
        } else if let Some(sig) = self.signal {
            format!("killed by {}", signal_name(sig))
        } else {
            "did not run".into()
        }
    }
}

pub fn signal_name(sig: i32) -> String {
    #[cfg(unix)]
    {
        let name = match sig {
            libc::SIGINT => "SIGINT",
            libc::SIGKILL => "SIGKILL",
            libc::SIGPIPE => "SIGPIPE",
            libc::SIGSEGV => "SIGSEGV",
            libc::SIGABRT => "SIGABRT",
            libc::SIGBUS => "SIGBUS",
            libc::SIGTERM => "SIGTERM",
            libc::SIGILL => "SIGILL",
            libc::SIGFPE => "SIGFPE",
            libc::SIGTRAP => "SIGTRAP",
            _ => return format!("signal {sig}"),
        };
        name.to_string()
    }
    #[cfg(not(unix))]
    {
        format!("signal {sig}")
    }
}

/// Signals that mean the program crashed rather than being asked to stop.
pub fn is_crash_signal(sig: i32) -> bool {
    #[cfg(unix)]
    {
        [
            libc::SIGSEGV,
            libc::SIGABRT,
            libc::SIGBUS,
            libc::SIGILL,
            libc::SIGFPE,
            libc::SIGTRAP,
        ]
        .contains(&sig)
    }
    #[cfg(not(unix))]
    {
        let _ = sig;
        false
    }
}

pub fn run(target: &Target, spec: &Spec) -> Outcome {
    match spec.mode {
        Mode::Pipe => run_piped(target, spec, false),
        Mode::ClosedStdout => run_piped(target, spec, true),
        Mode::Pty {
            cols,
            rows,
            ctrl_c_after,
        } => pty::run(target, spec, cols, rows, ctrl_c_after),
    }
}

fn base_command(target: &Target, spec: &Spec) -> Command {
    let mut cmd = Command::new(&target.program);
    cmd.args(&target.prefix)
        .args(&spec.args)
        .current_dir(&spec.cwd);
    // Variables that force color on, or that would make a probe wait on a pager.
    for key in [
        "FORCE_COLOR",
        "CLICOLOR_FORCE",
        "CLICOLOR",
        "NO_COLOR",
        "CI",
        "COLUMNS",
        "LINES",
    ] {
        cmd.env_remove(key);
    }
    for key in ["PAGER", "GIT_PAGER", "MANPAGER"] {
        cmd.env(key, "cat");
    }
    cmd.env("TERM", "xterm-256color");
    for (key, value) in &spec.env {
        match value {
            Some(v) => cmd.env(key, v),
            None => cmd.env_remove(key),
        };
    }
    cmd
}

fn spawn_error(err: std::io::Error, target: &Target) -> Outcome {
    let msg = if err.kind() == std::io::ErrorKind::NotFound {
        format!("command not found: {}", target.program)
    } else {
        format!("could not start {}: {err}", target.program)
    };
    Outcome {
        spawn_error: Some(msg),
        ..Outcome::default()
    }
}

fn read_capped(mut reader: impl Read) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let room = MAX_CAPTURE.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    kept
}

/// Reads a stream on its own thread so a full pipe never blocks the child.
fn reader_thread(stream: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(stream) = stream {
        thread::spawn(move || {
            let _ = tx.send(read_capped(stream));
        });
    } else {
        let _ = tx.send(Vec::new());
    }
    rx
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn run_piped(target: &Target, spec: &Spec, closed_stdout: bool) -> Outcome {
    let mut cmd = base_command(target, spec);
    cmd.stdin(Stdio::null()).stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so a timeout can stop the tool and anything it started.
        cmd.process_group(0);
    }

    if closed_stdout {
        #[cfg(unix)]
        match closed_pipe() {
            Ok(write_end) => {
                cmd.stdout(Stdio::from(write_end));
            }
            Err(err) => {
                return Outcome {
                    spawn_error: Some(format!("could not create a pipe: {err}")),
                    ..Outcome::default()
                };
            }
        }
        #[cfg(not(unix))]
        return Outcome {
            unsupported: true,
            ..Outcome::default()
        };
    } else {
        cmd.stdout(Stdio::piped());
    }

    let start = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => return spawn_error(err, target),
    };
    // The command holds the only other copy of the pipe's write end; drop ours.
    drop(cmd);
    let out_rx = reader_thread(child.stdout.take());
    let err_rx = reader_thread(child.stderr.take());
    let pid = child.id();

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let status = child.wait();
        let _ = tx.send((status, Instant::now()));
    });

    let mut outcome = Outcome::default();
    let (status, ended) = match rx.recv_timeout(spec.timeout) {
        Ok(r) => r,
        Err(_) => {
            outcome.timed_out = true;
            kill_group(pid);
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(r) => r,
                Err(_) => (Err(std::io::Error::other("did not stop")), Instant::now()),
            }
        }
    };
    outcome.duration = ended.duration_since(start);
    if let Ok(status) = status {
        outcome.code = status.code();
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            outcome.signal = status.signal();
        }
    }
    // Grandchildren can keep a pipe open after the tool exits; don't wait on them forever.
    let grace = Duration::from_millis(if outcome.timed_out { 200 } else { 1500 });
    outcome.stdout = text(out_rx.recv_timeout(grace).unwrap_or_default());
    outcome.stderr = text(err_rx.recv_timeout(grace).unwrap_or_default());
    outcome
}

#[cfg(unix)]
fn closed_pipe() -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two descriptors pipe() writes.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: pipe() succeeded, so both descriptors are open and owned by us.
    let (read_end, write_end) =
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    drop(read_end);
    Ok(write_end)
}

fn kill_group(pid: u32) {
    #[cfg(unix)]
    // SAFETY: plain syscall; a stale pid only makes it fail with ESRCH.
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(unix)]
mod pty {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::sync::{Arc, Mutex};

    fn cloexec_dup(fd: RawFd) -> std::io::Result<OwnedFd> {
        // SAFETY: F_DUPFD_CLOEXEC on a descriptor we own returns a new one or -1.
        let new = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
        if new < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fcntl just created this descriptor for us.
        Ok(unsafe { OwnedFd::from_raw_fd(new) })
    }

    // glibc takes `*const winsize`; macOS/BSD take `*mut winsize`.
    #[allow(clippy::unnecessary_mut_passed)]
    fn open_pty(cols: u16, rows: u16) -> std::io::Result<(OwnedFd, OwnedFd)> {
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let mut size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: valid out-pointers; null name and termios mean "defaults".
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openpty succeeded, both descriptors are ours.
        let (master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        // Keep them out of the child (it gets dup2'd copies of the slave on 0, 1, 2).
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            // SAFETY: setting a flag on a descriptor we own.
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        Ok((master, slave))
    }

    /// Answers the questions programs ask a terminal, the way a real one would.
    /// Without this, tools that query the background color or cursor position
    /// (common in Go and Rust TUI libraries) wait forever for a reply.
    fn reply_to(query: &[u8]) -> Option<(usize, Vec<u8>)> {
        let fixed: [(&[u8], &[u8]); 8] = [
            (b"\x1b[6n", b"\x1b[1;1R"),
            (b"\x1b[?6n", b"\x1b[?1;1R"),
            (b"\x1b[5n", b"\x1b[0n"),
            (b"\x1b[c", b"\x1b[?62;22c"),
            (b"\x1b[0c", b"\x1b[?62;22c"),
            (b"\x1b[>c", b"\x1b[>0;0;0c"),
            (b"\x1b[>0c", b"\x1b[>0;0;0c"),
            (b"\x1b[?u", b"\x1b[?0u"),
        ];
        for (q, r) in fixed {
            if query.starts_with(q) {
                return Some((q.len(), r.to_vec()));
            }
        }
        // OSC 10/11/12 color queries: ESC ] 11 ; ? (BEL | ESC \)
        for (num, color) in [
            (b"10", "ffff/ffff/ffff"),
            (b"11", "0000/0000/0000"),
            (b"12", "ffff/ffff/ffff"),
        ] {
            let mut q = b"\x1b]".to_vec();
            q.extend_from_slice(num);
            q.extend_from_slice(b";?");
            if query.starts_with(&q) {
                let rest = &query[q.len()..];
                let (term_len, term): (usize, &[u8]) = if rest.first() == Some(&0x07) {
                    (1, b"\x07")
                } else if rest.starts_with(b"\x1b\\") {
                    (2, b"\x1b\\")
                } else {
                    return None;
                };
                let mut r = format!(
                    "\x1b]{};rgb:{color}",
                    std::str::from_utf8(num).unwrap_or("11")
                )
                .into_bytes();
                r.extend_from_slice(term);
                return Some((q.len() + term_len, r));
            }
        }
        // DECRQM, "is mode N supported?": ESC [ ? N $ p  ->  ESC [ ? N ; 0 $ y
        if let Some(rest) = query.strip_prefix(b"\x1b[?") {
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits > 0 && rest[digits..].starts_with(b"$p") {
                let mut r = b"\x1b[?".to_vec();
                r.extend_from_slice(&rest[..digits]);
                r.extend_from_slice(b";0$y");
                return Some((3 + digits + 2, r));
            }
        }
        // Window size in characters: ESC [ 18 t
        if query.starts_with(b"\x1b[18t") {
            return Some((5, b"\x1b[8;30;100t".to_vec()));
        }
        None
    }

    /// Reads everything the tool shows, answering terminal queries as they arrive.
    fn screen_reader(
        mut master: File,
        mut replies: File,
    ) -> (Arc<Mutex<Vec<u8>>>, mpsc::Receiver<()>) {
        let screen = Arc::new(Mutex::new(Vec::new()));
        let (done_tx, done_rx) = mpsc::channel();
        let shared = Arc::clone(&screen);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut pending: Vec<u8> = Vec::new();
            loop {
                let n = match master.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                if let Ok(mut s) = shared.lock() {
                    let room = MAX_CAPTURE.saturating_sub(s.len());
                    s.extend_from_slice(&buf[..n.min(room)]);
                }
                pending.extend_from_slice(&buf[..n]);
                let mut i = 0;
                let mut answered_to = 0;
                while i < pending.len() {
                    if pending[i] == 0x1b
                        && let Some((len, reply)) = reply_to(&pending[i..])
                    {
                        let _ = replies.write_all(&reply);
                        let _ = replies.flush();
                        i += len;
                        answered_to = i;
                        continue;
                    }
                    i += 1;
                }
                // Keep a short tail in case a query is split across two reads.
                let keep_from = answered_to.max(pending.len().saturating_sub(24));
                pending.drain(..keep_from);
            }
            let _ = done_tx.send(());
        });
        (screen, done_rx)
    }

    fn snapshot(screen: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
        screen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn clean(raw: Vec<u8>) -> String {
        text(raw).replace("\r\n", "\n").replace('\r', "\n")
    }

    pub fn run(
        target: &Target,
        spec: &Spec,
        cols: u16,
        rows: u16,
        ctrl_c_after: Option<Duration>,
    ) -> Outcome {
        let fail = |e: std::io::Error| Outcome {
            spawn_error: Some(format!("could not open a terminal: {e}")),
            ..Outcome::default()
        };
        let (master, slave) = match open_pty(cols, rows) {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        let stdio = || cloexec_dup(slave.as_raw_fd()).map(|fd| Stdio::from(File::from(fd)));
        let (sin, sout, serr) = match (stdio(), stdio(), stdio()) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return fail(e),
        };
        let (keys, replies) = match (
            cloexec_dup(master.as_raw_fd()),
            cloexec_dup(master.as_raw_fd()),
        ) {
            (Ok(a), Ok(b)) => (File::from(a), File::from(b)),
            (Err(e), _) | (_, Err(e)) => return fail(e),
        };

        let mut cmd = base_command(target, spec);
        cmd.stdin(sin).stdout(sout).stderr(serr);
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                // New session with the terminal as its controlling tty, so Ctrl-C
                // typed into it reaches the tool as SIGINT, like in a real terminal.
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let start = Instant::now();
        let child = cmd.spawn();
        drop(cmd);
        drop(slave);
        let mut child = match child {
            Ok(c) => c,
            Err(err) => return spawn_error(err, target),
        };
        let pid = child.id();
        let mut keys = keys;
        let (screen, reader_done) = screen_reader(File::from(master), replies);

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let status = child.wait();
            let _ = tx.send((status, Instant::now()));
        });

        let mut outcome = Outcome::default();
        let first_wait = ctrl_c_after.unwrap_or(spec.timeout).min(spec.timeout);
        let mut result = rx.recv_timeout(first_wait).ok();

        if result.is_none() && ctrl_c_after.is_some() {
            outcome.ctrl_c_sent = true;
            let before = snapshot(&screen);
            outcome.before_ctrl_c = clean(before.clone());
            // Ctrl-C, then (for REPLs, which use Ctrl-C to cancel a line) a second
            // Ctrl-C, then Ctrl-D, which ends input.
            for (stage, key, wait) in [(1u8, 0x03u8, 1500u64), (2, 0x03, 1000), (3, 0x04, 1000)] {
                let sent_at = Instant::now();
                let _ = keys.write_all(&[key]);
                let _ = keys.flush();
                if let Ok(r) = rx.recv_timeout(Duration::from_millis(wait)) {
                    outcome.ctrl_c_exit = Some(r.1.duration_since(sent_at));
                    outcome.exit_stage = Some(stage);
                    result = Some(r);
                    break;
                }
                if stage == 1 {
                    // Anything new on screen (beyond the echoed ^C) counts as a reaction.
                    let after = snapshot(&screen);
                    let added = String::from_utf8_lossy(&after[before.len().min(after.len())..])
                        .replace("^C", "");
                    outcome.ctrl_c_reacted = !added.trim().is_empty();
                }
            }
        } else if result.is_none() {
            result = rx
                .recv_timeout(spec.timeout.saturating_sub(first_wait))
                .ok();
        }

        let (status, ended) = match result {
            Some(r) => r,
            None => {
                outcome.timed_out = true;
                kill_group(pid);
                rx.recv_timeout(Duration::from_secs(2))
                    .unwrap_or((Err(std::io::Error::other("did not stop")), Instant::now()))
            }
        };
        outcome.duration = ended.duration_since(start);
        if let Ok(status) = status {
            outcome.code = status.code();
            outcome.signal = status.signal();
        }
        drop(keys);
        // Something the tool started may still hold the terminal open.
        let grace = Duration::from_millis(if outcome.timed_out { 300 } else { 1500 });
        let _ = reader_done.recv_timeout(grace);
        outcome.stdout = clean(snapshot(&screen));
        outcome
    }
}

#[cfg(not(unix))]
mod pty {
    use super::*;

    pub fn run(_: &Target, _: &Spec, _: u16, _: u16, _: Option<Duration>) -> Outcome {
        Outcome {
            unsupported: true,
            ..Outcome::default()
        }
    }
}

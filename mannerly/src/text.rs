//! Reading the text a tool printed: escape codes, help sections, stack traces.

/// What kinds of escape sequences appear in some output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Escapes {
    /// Any ESC byte at all.
    pub any: bool,
    /// SGR sequences that set a foreground or background color.
    pub color: bool,
}

pub fn escapes(s: &str) -> Escapes {
    let bytes = s.as_bytes();
    let mut found = Escapes::default();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            i += 1;
            continue;
        }
        found.any = true;
        if bytes.get(i + 1) == Some(&b'[') {
            // CSI: parameters, then a final byte in 0x40..=0x7e.
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'm' && sgr_sets_color(&s[start..j]) {
                found.color = true;
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    found
}

fn sgr_sets_color(params: &str) -> bool {
    // The first number decides; "38;5;196" is still a color, "1;31" has one later.
    params
        .split([';', ':'])
        .filter_map(|p| p.parse::<u16>().ok())
        .any(|n| matches!(n, 30..=38 | 40..=48 | 58 | 90..=97 | 100..=107))
}

/// Removes escape sequences so widths and words can be measured.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC, ended by BEL or ESC \.
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Display width of a line, counting wide (CJK, emoji) characters as two columns.
pub fn width(line: &str) -> usize {
    line.chars()
        .map(|c| match c as u32 {
            0..=0x1f | 0x7f..=0x9f | 0x300..=0x36f | 0x200b..=0x200f | 0xfe00..=0xfe0f => 0,
            0x1100..=0x115f
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe30..=0xfe4f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1f64f
            | 0x1f900..=0x1f9ff
            | 0x20000..=0x3fffd => 2,
            _ => 1,
        })
        .sum()
}

pub fn max_width(s: &str) -> usize {
    strip_ansi(s).lines().map(width).max().unwrap_or(0)
}

pub fn lines_wider_than(s: &str, cols: usize) -> usize {
    strip_ansi(s).lines().filter(|l| width(l) > cols).count()
}

/// A stack trace, panic or crash report from a common runtime, if the output has one.
pub fn traceback(s: &str) -> Option<&'static str> {
    let markers: [(&str, &str); 9] = [
        ("Traceback (most recent call last)", "a Python traceback"),
        ("panicked at", "a Rust panic"),
        ("goroutine 1 [", "a Go panic"),
        ("Exception in thread \"main\"", "a Java stack trace"),
        ("Unhandled exception.", "a .NET stack trace"),
        ("Segmentation fault", "a segfault"),
        ("core dumped", "a crash"),
        ("node:internal/", "a Node.js stack trace"),
        ("UnhandledPromiseRejection", "a Node.js stack trace"),
    ];
    for (marker, what) in markers {
        if s.contains(marker) {
            return Some(what);
        }
    }
    let mut js_frames = 0;
    for line in s.lines() {
        let t = line.trim_start();
        // V8: "    at fn (/path/file.js:12:5)"
        if line.starts_with("    at ") && t.ends_with(')') && has_line_col(t) {
            js_frames += 1;
        }
        // Ruby: "/path/file.rb:12:in `method'"
        if t.contains(".rb:") && t.contains(":in ") {
            return Some("a Ruby backtrace");
        }
        if t.starts_with("panic: ") {
            return Some("a Go panic");
        }
    }
    if js_frames >= 2 {
        return Some("a JavaScript stack trace");
    }
    None
}

fn has_line_col(s: &str) -> bool {
    let inner = s.trim_end_matches(')');
    let mut parts = inner.rsplit(':');
    let col = parts.next().unwrap_or("");
    let line = parts.next().unwrap_or("");
    !col.is_empty()
        && !line.is_empty()
        && col.chars().all(|c| c.is_ascii_digit())
        && line.chars().all(|c| c.is_ascii_digit())
}

/// What we can learn from a tool's `--help` text.
#[derive(Clone, Debug, Default)]
pub struct Help {
    pub has_usage: bool,
    pub has_examples: bool,
    pub has_url: bool,
    pub machine_output: bool,
    pub subcommands: Vec<String>,
}

pub fn parse_help(raw: &str) -> Help {
    let s = strip_ansi(raw);
    let lower = s.to_lowercase();
    let lines: Vec<&str> = s.lines().collect();

    let has_usage = lines.iter().any(|l| {
        let t = l.trim().to_lowercase();
        t.starts_with("usage")
            || t.contains("usage:")
            || t == "synopsis"
            || t.starts_with("synopsis:")
    });
    let has_examples = lower.contains("example")
        || lines
            .iter()
            .any(|l| l.trim_start().starts_with("$ ") || l.trim_start().starts_with("> "));
    let has_url = lower.contains("https://") || lower.contains("http://");
    let machine_output = [
        "--json",
        "--format",
        "--output",
        "--porcelain",
        "--template",
        "--jq",
        "--plain",
        "--csv",
        "--ndjson",
        "--yaml",
        "-o json",
        "--message-format",
    ]
    .iter()
    .any(|f| lower.contains(f));

    Help {
        has_usage,
        has_examples,
        has_url,
        machine_output,
        subcommands: subcommands(&lines),
    }
}

fn is_command_header(line: &str) -> bool {
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }
    let t = line.trim().trim_end_matches(':').to_lowercase();
    if t.is_empty() || t.len() > 80 {
        return false;
    }
    t == "commands"
        || t == "subcommands"
        || t.ends_with(" commands")
        || t.ends_with(" subcommands")
        || t.starts_with("commands ")
        || t.starts_with("available commands")
        || (t.contains("commands") && line.trim_end().ends_with(':'))
}

fn is_other_header(line: &str) -> bool {
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }
    let t = line.trim().trim_end_matches(':').to_lowercase();
    [
        "options",
        "flags",
        "global options",
        "global flags",
        "arguments",
        "args",
        "environment",
        "environment variables",
        "examples",
        "example",
        "see also",
        "learn more",
        "usage",
        "positional arguments",
        "optional arguments",
    ]
    .iter()
    .any(|h| t == *h || t.starts_with(&format!("{h} ")))
}

fn looks_like_command(word: &str) -> bool {
    let w = word.trim_end_matches([',', ':']);
    (2..=30).contains(&w.len())
        && w.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && w.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Command names listed under a "Commands:"-style heading.
fn subcommands(lines: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut in_section = false;
    for line in lines {
        if is_command_header(line) {
            in_section = true;
            continue;
        }
        if is_other_header(line) {
            in_section = false;
            continue;
        }
        if !in_section || line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with("  ") || line.starts_with('\t');
        if !indented {
            continue;
        }
        let trimmed = line.trim();
        // "access, adduser, audit," style lists.
        if trimmed.contains(", ") && !trimmed.contains("  ") {
            for w in trimmed.split(',').map(str::trim) {
                if looks_like_command(w) {
                    push_unique(&mut found, w);
                }
            }
            continue;
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        // "build, b   Compile ..." lists an alias after the name.
        let name = first.trim_end_matches(',');
        if looks_like_command(name) {
            push_unique(&mut found, name);
        }
    }
    found
}

fn push_unique(list: &mut Vec<String>, word: &str) {
    let w = word.trim_end_matches([',', ':']).to_string();
    if !list.contains(&w) {
        list.push(w);
    }
}

/// A plausible typo of `word` that isn't one of the real commands.
pub fn typo_of(word: &str, commands: &[String]) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    if chars.len() < 4 {
        return None;
    }
    for i in 1..chars.len() - 1 {
        if chars[i] == chars[i + 1] {
            continue;
        }
        let mut c = chars.clone();
        c.swap(i, i + 1);
        let candidate: String = c.into_iter().collect();
        if !commands.iter().any(|x| x == &candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Looks for something shaped like a version number: 1.2, v0.3.1, 2024.1.
pub fn has_version_number(s: &str) -> bool {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i].is_ascii_digit()
            && b.get(i + 1) == Some(&b'.')
            && b.get(i + 2).is_some_and(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_color_but_not_bold() {
        assert!(escapes("\x1b[31mred\x1b[0m").color);
        assert!(escapes("\x1b[1;38;5;196mx").color);
        let bold = escapes("\x1b[1mbold\x1b[0m");
        assert!(bold.any && !bold.color);
        assert_eq!(escapes("plain"), Escapes::default());
    }

    #[test]
    fn strips_csi_and_osc() {
        let s = "\x1b[1mhi\x1b[0m \x1b]8;;https://x\x07link\x1b]8;;\x07";
        assert_eq!(strip_ansi(s), "hi link");
    }

    #[test]
    fn spots_tracebacks() {
        assert!(traceback("Traceback (most recent call last):\n  File").is_some());
        assert!(traceback("thread 'main' panicked at src/main.rs:3:5").is_some());
        let js = "Error: boom\n    at run (/a/b.js:1:2)\n    at main (/a/c.js:3:4)\n";
        assert!(traceback(js).is_some());
        assert!(traceback("error: unexpected argument '--x' found").is_none());
    }

    #[test]
    fn parses_clap_style_help() {
        let help = "Does things\n\nUsage: tool [OPTIONS] <COMMAND>\n\nCommands:\n  build, b  Compile\n  run       Run it\n  help      Print help\n\nOptions:\n  -q, --quiet  Shh\n  --json  JSON\n";
        let h = parse_help(help);
        assert!(h.has_usage && h.machine_output && !h.has_examples);
        assert_eq!(h.subcommands, ["build", "run", "help"]);
    }

    #[test]
    fn parses_git_style_help() {
        let help = "usage: git [-v | --version]\n\nThese are common Git commands used in various situations:\n\nstart a working area (see also: git help tutorial)\n   clone     Clone a repository\n   init      Create an empty Git repository\n\nwork on the current change\n   add       Add file contents\n";
        let h = parse_help(help);
        assert_eq!(h.subcommands, ["clone", "init", "add"]);
    }

    #[test]
    fn makes_typos() {
        let cmds = vec!["build".to_string()];
        assert_eq!(typo_of("build", &cmds).as_deref(), Some("biuld"));
        assert_eq!(typo_of("go", &cmds), None);
    }

    #[test]
    fn versions_and_widths() {
        assert!(has_version_number("tool 1.2.3"));
        assert!(!has_version_number("tool version unknown"));
        assert_eq!(width("日本"), 4);
        assert_eq!(max_width("ab\n\x1b[1mabcd\x1b[0m"), 4);
    }
}

//! Turning results into something to read: terminal, JSON, Markdown, badge.

use crate::checks::{Category, CheckResult, DEFS, Report, Status};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Clone, Copy)]
pub struct Style {
    pub color: bool,
    pub width: usize,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
    fn yellow(&self, s: &str) -> String {
        self.paint("33", s)
    }
    fn red(&self, s: &str) -> String {
        self.paint("31", s)
    }
    fn for_score(&self, score: u32, s: &str) -> String {
        if score >= 90 {
            self.green(s)
        } else if score >= 50 {
            self.yellow(s)
        } else {
            self.red(s)
        }
    }
}

pub fn verdict(score: u32) -> &'static str {
    match score {
        100 => "Impeccable manners.",
        90..=99 => "Good manners.",
        75..=89 => "Decent, with a few rough edges.",
        50..=74 => "Could use some etiquette lessons.",
        _ => "Rude. Let's fix that.",
    }
}

fn icon(style: &Style, status: Status) -> String {
    match status {
        Status::Pass => style.green("✓"),
        Status::Warn => style.yellow("!"),
        Status::Fail => style.red("✗"),
        Status::Skip => style.dim("–"),
    }
}

fn bar(style: &Style, score: u32, cells: usize) -> String {
    let filled = (score as usize * cells + 50) / 100;
    let full = "━".repeat(filled);
    let empty = "─".repeat(cells - filled);
    format!("{}{}", style.for_score(score, &full), style.dim(&empty))
}

/// Word-wraps `text` to `width` columns, indenting every line after the first.
fn wrap(text: &str, width: usize, indent: usize) -> String {
    let width = width.max(indent + 20);
    let mut out = String::new();
    let mut col = indent;
    for word in text.split_whitespace() {
        let w = crate::text::width(word);
        if col > indent && col + 1 + w > width {
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            col = indent;
        } else if col > indent {
            out.push(' ');
            col += 1;
        }
        out.push_str(word);
        col += w;
    }
    out
}

pub fn terminal(report: &Report, style: &Style, verbose: bool) -> String {
    let mut o = String::new();
    let score = report.score();
    let w = style.width.clamp(40, 100);

    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "  {}  {}",
        style.bold(&style.for_score(score, &format!("{score:>3}"))),
        style.bold(&report.command)
    );
    let _ = writeln!(o, "       {}", verdict(score));
    let _ = writeln!(o);

    for cat in Category::ALL {
        let name = format!("{:<11}", cat.name());
        match report.category_score(cat) {
            Some(s) => {
                let _ = writeln!(o, "  {name} {:>3}  {}", s, bar(style, s, 20));
            }
            None => {
                let _ = writeln!(o, "  {name}   {}", style.dim("–  nothing to check"));
            }
        }
    }

    let mut shown: Vec<&CheckResult> = report
        .results
        .iter()
        .filter(|r| r.status == Status::Fail)
        .collect();
    shown.extend(report.results.iter().filter(|r| r.status == Status::Warn));
    if verbose {
        shown.extend(report.results.iter().filter(|r| r.status == Status::Pass));
        shown.extend(report.results.iter().filter(|r| r.status == Status::Skip));
    }

    if !shown.is_empty() {
        let _ = writeln!(o);
    }
    let id_width = shown.iter().map(|r| r.def.id.len()).max().unwrap_or(0);
    let indent = 4 + id_width + 2;
    for r in shown {
        let _ = writeln!(
            o,
            "  {} {:<id_width$}  {}",
            icon(style, r.status),
            r.def.id,
            style.bold(r.def.title)
        );
        let _ = writeln!(o, "{}{}", " ".repeat(indent), wrap(&r.detail, w, indent));
        if matches!(r.status, Status::Fail | Status::Warn) {
            let fix = format!("Fix: {}", r.def.fix);
            // Style each line on its own, so pagers that reset per line keep it.
            let lines: Vec<String> = wrap(&fix, w, indent)
                .split('\n')
                .map(|l| {
                    let body = l.trim_start();
                    format!("{}{}", &l[..l.len() - body.len()], style.dim(body))
                })
                .collect();
            let _ = writeln!(o, "{}{}", " ".repeat(indent), lines.join("\n"));
        }
    }

    let _ = writeln!(o);
    let mut parts = vec![style.green(&format!("{} passed", report.count(Status::Pass)))];
    let fails = report.count(Status::Fail);
    if fails > 0 {
        parts.push(style.red(&format!("{fails} failed")));
    }
    let warns = report.count(Status::Warn);
    if warns > 0 {
        let s = if warns == 1 { "" } else { "s" };
        parts.push(style.yellow(&format!("{warns} warning{s}")));
    }
    let skips = report.count(Status::Skip);
    if skips > 0 {
        parts.push(style.dim(&format!("{skips} skipped")));
    }
    let secs = report.duration_ms as f64 / 1000.0;
    let _ = writeln!(
        o,
        "  {}  {}",
        parts.join(" · "),
        style.dim(&format!("in {secs:.1}s"))
    );
    if !verbose && (fails + warns) > 0 {
        let _ = writeln!(
            o,
            "  {}",
            style.dim("Run with -v to see every check, or --explain <id> for details.")
        );
    }
    o
}

#[derive(Serialize)]
struct JsonCheck<'a> {
    id: &'a str,
    category: Category,
    title: &'a str,
    status: Status,
    weight: u32,
    detail: &'a str,
    why: &'a str,
    fix: &'a str,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    mannerly: &'a str,
    command: &'a str,
    score: u32,
    verdict: &'a str,
    categories: BTreeMap<&'a str, Option<u32>>,
    passed: usize,
    failed: usize,
    warnings: usize,
    skipped: usize,
    duration_ms: u128,
    checks: Vec<JsonCheck<'a>>,
}

pub fn json(report: &Report) -> String {
    let score = report.score();
    let r = JsonReport {
        mannerly: env!("CARGO_PKG_VERSION"),
        command: &report.command,
        score,
        verdict: verdict(score),
        categories: Category::ALL
            .iter()
            .map(|c| (c.name(), report.category_score(*c)))
            .map(|(n, s)| (lowercase_static(n), s))
            .collect(),
        passed: report.count(Status::Pass),
        failed: report.count(Status::Fail),
        warnings: report.count(Status::Warn),
        skipped: report.count(Status::Skip),
        duration_ms: report.duration_ms,
        checks: report
            .results
            .iter()
            .map(|r| JsonCheck {
                id: r.def.id,
                category: r.def.category,
                title: r.def.title,
                status: r.status,
                weight: r.def.weight,
                detail: &r.detail,
                why: r.def.why,
                fix: r.def.fix,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&r).unwrap_or_default() + "\n"
}

fn lowercase_static(name: &str) -> &'static str {
    match name {
        "Help" => "help",
        "Errors" => "errors",
        "Output" => "output",
        _ => "robustness",
    }
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn markdown(report: &Report) -> String {
    let mut o = String::new();
    let score = report.score();
    let _ = writeln!(
        o,
        "### mannerly: `{}` scores {score}/100\n\n{}\n",
        report.command,
        verdict(score)
    );
    let _ = writeln!(
        o,
        "| Help | Errors | Output | Robustness |\n|---|---|---|---|"
    );
    let cell = |c| {
        report
            .category_score(c)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "–".into())
    };
    let _ = writeln!(
        o,
        "| {} | {} | {} | {} |\n",
        cell(Category::Help),
        cell(Category::Errors),
        cell(Category::Output),
        cell(Category::Robustness)
    );
    let issues: Vec<&CheckResult> = report
        .results
        .iter()
        .filter(|r| r.status == Status::Fail)
        .chain(report.results.iter().filter(|r| r.status == Status::Warn))
        .collect();
    if !issues.is_empty() {
        let _ = writeln!(o, "| | Check | Result | Fix |\n|---|---|---|---|");
        for r in issues {
            let mark = if r.status == Status::Fail {
                "❌"
            } else {
                "⚠️"
            };
            let _ = writeln!(
                o,
                "| {mark} | {} <br><sub>`{}`</sub> | {} | {} |",
                md_escape(r.def.title),
                r.def.id,
                md_escape(&r.detail),
                md_escape(r.def.fix)
            );
        }
        let _ = writeln!(o);
    }
    let passed: Vec<&CheckResult> = report
        .results
        .iter()
        .filter(|r| r.status == Status::Pass)
        .collect();
    let _ = writeln!(o, "<details><summary>{} passed</summary>\n", passed.len());
    for r in passed {
        let _ = writeln!(o, "- ✅ {} — {}", r.def.title, md_escape(&r.detail));
    }
    let _ = writeln!(o, "\n</details>");
    o
}

/// Rough Verdana 11px advance widths, enough to size a badge.
fn text_width(s: &str) -> f64 {
    s.chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | '.' | ',' | ':' | '\'' | '|' | '!' => 3.5,
            'f' | 't' | 'r' | 'I' | '/' | ' ' => 4.6,
            'm' | 'w' | 'M' | 'W' => 10.5,
            'A'..='Z' => 7.6,
            _ => 7.0,
        })
        .sum()
}

pub fn badge(score: u32) -> String {
    let label = "manners";
    let value = format!("{score}/100");
    let color = match score {
        90..=100 => "#4c1",
        75..=89 => "#97ca00",
        60..=74 => "#dfb317",
        40..=59 => "#fe7d37",
        _ => "#e05d44",
    };
    let lw = text_width(label) + 12.0;
    let vw = text_width(&value) + 12.0;
    let total = lw + vw;
    let lx = lw / 2.0;
    let vx = lw + vw / 2.0;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total:.0}" height="20" role="img" aria-label="{label}: {value}">
  <title>{label}: {value}</title>
  <linearGradient id="s" x2="0" y2="100%"><stop offset="0" stop-color="#bbb" stop-opacity=".1"/><stop offset="1" stop-opacity=".1"/></linearGradient>
  <clipPath id="r"><rect width="{total:.0}" height="20" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{lw:.0}" height="20" fill="#555"/>
    <rect x="{lw:.0}" width="{vw:.0}" height="20" fill="{color}"/>
    <rect width="{total:.0}" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="{lx:.1}" y="15" fill="#010101" fill-opacity=".3">{label}</text>
    <text x="{lx:.1}" y="14">{label}</text>
    <text x="{vx:.1}" y="15" fill="#010101" fill-opacity=".3">{value}</text>
    <text x="{vx:.1}" y="14">{value}</text>
  </g>
</svg>
"##
    )
}

pub fn list_checks(style: &Style) -> String {
    let mut o = String::new();
    for cat in Category::ALL {
        let _ = writeln!(o, "{}", style.bold(cat.name()));
        for d in DEFS.iter().filter(|d| d.category == cat) {
            let _ = writeln!(o, "  {:<20} {:>2}  {}", d.id, d.weight, d.title);
        }
        let _ = writeln!(o);
    }
    let _ = writeln!(
        o,
        "{}",
        style.dim("The number is the check's weight in the score. Run --explain <id> for details.")
    );
    o
}

pub fn explain(id: &str, style: &Style) -> Option<String> {
    let d = DEFS.iter().find(|d| d.id == id)?;
    let w = style.width.clamp(40, 100);
    Some(format!(
        "{}  {}\n{} · weight {}\n\n{}\n\n{} {}\n",
        style.bold(d.id),
        d.title,
        d.category.name(),
        d.weight,
        wrap(d.why, w, 0),
        style.bold("Fix:"),
        wrap(d.fix, w, 5)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_with_indent() {
        let s = wrap("one two three four five six", 30, 10);
        for line in s.lines().skip(1) {
            assert!(line.starts_with("          "));
        }
        assert!(s.lines().all(|l| crate::text::width(l) <= 30));
    }

    #[test]
    fn badge_is_svg() {
        let b = badge(93);
        assert!(b.starts_with("<svg") && b.contains("93/100") && b.contains("#4c1"));
    }

    #[test]
    fn every_check_explains() {
        let style = Style {
            color: false,
            width: 80,
        };
        for d in DEFS {
            assert!(explain(d.id, &style).is_some());
            assert!(d.why.len() > 20 && d.fix.len() > 10, "{}", d.id);
        }
    }
}

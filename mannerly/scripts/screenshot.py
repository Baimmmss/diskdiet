#!/usr/bin/env python3
"""Renders a real mannerly run as an SVG terminal screenshot.

    python3 scripts/screenshot.py assets/screenshot.svg -- pip3
"""
import html
import os
import re
import subprocess
import sys

FG, BG = "#e4e4e7", "#0f1117"
COLORS = {"31": "#fca5a5", "32": "#86efac", "33": "#fde68a"}
CW, LH, FS, PAD, TOP = 8.43, 20, 14, 22, 44


def runs(line):
    """Splits one line of ANSI text into (text, bold, dim, color) runs."""
    bold = dim = False
    color = None
    pos = 0
    for m in re.finditer(r"\x1b\[([0-9;]*)m", line):
        if m.start() > pos:
            yield line[pos:m.start()], bold, dim, color
        for code in (m.group(1) or "0").split(";"):
            if code == "0":
                bold = dim = False
                color = None
            elif code == "1":
                bold = True
            elif code == "2":
                dim = True
            elif code in COLORS:
                color = COLORS[code]
        pos = m.end()
    if pos < len(line):
        yield line[pos:], bold, dim, color


def main():
    out_path = sys.argv[1]
    cmd = sys.argv[sys.argv.index("--") + 1:]
    exe = os.environ.get("MANNERLY", "./target/release/mannerly")
    env = dict(os.environ, COLUMNS="92")
    text = subprocess.run([exe, "--color", "always", *cmd], capture_output=True, text=True, env=env).stdout
    lines = [f"$ mannerly {' '.join(cmd)}"] + text.rstrip("\n").split("\n")
    cols = max(len(re.sub(r"\x1b\[[0-9;]*m", "", l)) for l in lines)
    width = round(PAD * 2 + cols * CW)
    height = TOP + len(lines) * LH + PAD
    svg = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        f'<rect width="100%" height="100%" rx="10" fill="{BG}"/>',
        '<circle cx="20" cy="17" r="6" fill="#ff5f57"/><circle cx="40" cy="17" r="6" fill="#febc2e"/><circle cx="60" cy="17" r="6" fill="#28c840"/>',
        f'<g font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,monospace" font-size="{FS}" xml:space="preserve">',
    ]
    for i, line in enumerate(lines):
        y = TOP + i * LH + FS
        col = 0
        if i == 0:
            svg.append(f'<text x="{PAD}" y="{y}" fill="#a1a1aa">{html.escape(line)}</text>')
            continue
        for chunk, bold, dim, color in runs(line):
            attrs = f'fill="{color or FG}"'
            if bold:
                attrs += ' font-weight="700"'
            if dim:
                attrs += ' opacity="0.5"'
            # One <text> per word with every glyph placed, so no renderer can
            # collapse spaces or let a fallback glyph shift the columns.
            for m in re.finditer(r"\S+", chunk):
                word = m.group(0)
                xs = " ".join(f"{PAD + (col + m.start() + k) * CW:.1f}" for k in range(len(word)))
                svg.append(f'<text x="{xs}" y="{y}" {attrs}>{html.escape(word)}</text>')
            col += len(chunk)
    svg += ["</g>", "</svg>"]
    with open(out_path, "w") as f:
        f.write("\n".join(svg) + "\n")
    print(f"wrote {out_path} ({width}x{height})")


if __name__ == "__main__":
    main()

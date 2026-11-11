#!/bin/sh
# Prints a Markdown leaderboard of how well-known CLIs score.
#   sh scripts/leaderboard.sh git cargo gh jq npm > leaderboard.md
set -eu
MANNERLY="${MANNERLY:-./target/release/mannerly}"
echo "| Score | Command | Help | Errors | Output | Robustness | Failed checks |"
echo "|---:|---|---:|---:|---:|---:|---|"
for tool in "$@"; do
  command -v "$tool" >/dev/null 2>&1 || continue
  "$MANNERLY" --json "$tool" 2>/dev/null | python3 -c '
import json, sys
d = json.load(sys.stdin)
c = d["categories"]
cell = lambda k: "–" if c[k] is None else str(c[k])
failed = ", ".join("`%s`" % x["id"] for x in d["checks"] if x["status"] == "fail") or "–"
print("| %d | `%s` | %s | %s | %s | %s | %s |" % (d["score"], d["command"], cell("help"), cell("errors"), cell("output"), cell("robustness"), failed))
'
done | sort -t'|' -k2 -rn

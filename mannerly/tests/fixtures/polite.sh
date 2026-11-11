#!/bin/sh
# A small, well-behaved CLI: mannerly should give it a perfect score.
help() {
  cat <<'HELP'
polite: says hello, nicely

Usage: polite [OPTIONS] <NAME>

Options:
  -h, --help     Print help
  -V, --version  Print version
      --json     Print JSON

Examples:
  polite world
  polite --json world

Docs: https://example.com/polite
HELP
}
case "${1-}" in
  -h|--help) help; exit 0 ;;
  -V|--version) echo "polite 1.2.3"; exit 0 ;;
  "") echo "error: missing <NAME>" >&2; echo "Run 'polite --help' for usage." >&2; exit 2 ;;
  -*) echo "error: unknown option '$1'" >&2; echo "Run 'polite --help' for usage." >&2; exit 2 ;;
  *) echo "hello, $1"; exit 0 ;;
esac

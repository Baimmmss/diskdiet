#!/bin/sh
# Ignores Ctrl-C and waits forever when run with no arguments.
case "${1-}" in
  --help) echo "Usage: stubborn"; exit 0 ;;
  --version) echo "stubborn 0.1.0"; exit 0 ;;
  -*) echo "stubborn: unknown option $1" >&2; exit 2 ;;
esac
trap '' INT
while :; do sleep 1; done

#!/bin/sh
# A badly behaved CLI: color in pipes, exit 0 on bad flags, stack traces, litter.
case "${1-}" in
  --help|-h)
    printf '\033[1;31mRUDE TOOL\033[0m does things\n'
    touch rude.log
    exit 0 ;;
  --version) echo "rude version unknown"; exit 0 ;;
  -*)
    echo "Traceback (most recent call last):"
    echo "  File \"rude.py\", line 1, in <module>"
    echo "ValueError: bad"
    exit 0 ;;
  *) exit 0 ;;
esac

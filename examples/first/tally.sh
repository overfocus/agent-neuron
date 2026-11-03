#!/bin/sh
# Reads text on stdin and prints a JSON summary. POSIX sh, portable to macOS.
input=$(cat)
words=$(printf '%s' "$input" | wc -w | tr -d ' ')
lines=$(printf '%s\n' "$input" | wc -l | tr -d ' ')
printf '{"lines":%s,"words":%s}\n' "$lines" "$words"

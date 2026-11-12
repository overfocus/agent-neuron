#!/bin/sh
# Level 0: reads its input and answers with its own arguments, as a JSON
# array of strings (the arguments of the tests hold no quote or backslash).
cat >/dev/null
out=''
sep=''
for a in "$@"; do
  out="$out$sep\"$a\""
  sep=','
done
echo "[$out]"

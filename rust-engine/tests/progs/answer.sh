#!/bin/sh
# Level 0: reads one JSON value on stdin, prints one JSON answer with it.
read -r input || true
echo "{\"echo\": $input}"

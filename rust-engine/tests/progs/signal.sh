#!/bin/sh
# Level 0 that dies from a signal (SIGUSR1, 10 per POSIX): the error names the
# signal, never its number (6.1).
read -r input || true
kill -USR1 $$
sleep 5

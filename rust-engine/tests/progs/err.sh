#!/bin/sh
# Level 0: a non-zero exit with a message on stderr.
echo "boom on stderr" >&2
exit 7

#!/bin/sh
# Log detector. Usage: detect-logs.sh <log file>. Reads one alert as JSON on stdin.
# Flags it when the log file has ERROR lines for the alert's service; stays quiet otherwise.
alert=$(cat)
field() { printf '%s' "$alert" | sed -n "s/.*\"$1\":\"\{0,1\}\([^\",}]*\).*/\1/p"; }
service=$(field service)
severity=$(field severity)
[ "$severity" = info ] && exit 0
n=$(grep -c " ERROR $service " "$1")
[ "$n" -gt 0 ] || exit 0
printf '{"detector":"logs","alert":%s,"service":"%s","detail":"%s error lines in the log"}\n' "$(field id)" "$service" "$n"

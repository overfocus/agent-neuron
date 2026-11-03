#!/bin/sh
# Deploy detector. Usage: detect-deploys.sh <deploy list>. Reads one alert as JSON on stdin.
# Each deploy line is "<service> <deploy id> <minutes ago>". Flags the alert when its service
# was deployed in the last 30 minutes; stays quiet otherwise.
alert=$(cat)
field() { printf '%s' "$alert" | sed -n "s/.*\"$1\":\"\{0,1\}\([^\",}]*\).*/\1/p"; }
service=$(field service)
severity=$(field severity)
[ "$severity" = info ] && exit 0
awk -v s="$service" -v id="$(field id)" '$1 == s && $3 <= 30 {
  printf "{\"detector\":\"deploys\",\"alert\":%s,\"service\":\"%s\",\"detail\":\"deploy %s %s min ago\"}\n", id, s, $2, $3
}' "$1"

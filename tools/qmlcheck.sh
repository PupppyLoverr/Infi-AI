#!/bin/bash
# Parse every shell QML file and fail on any syntax problem. Quickshell stops
# at the first QML error, so this runs before every push of shell/qs.
# qmlformat's parser is the strict one (qmllint 6.4 lets some through, such as
# a bare `for` as a signal handler).
# Usage: tools/qmlcheck.sh [qml dir]   (default shell/qs)
dir="${1:-$(dirname "$0")/../shell/qs}"
fmt=$(command -v qmlformat || echo /usr/lib/qt6/bin/qmlformat)
bad=0
for f in "$dir"/*.qml; do
  out=$("$fmt" "$f" 2>&1 >/dev/null | grep -iE "error|invalid")
  [ -n "$out" ] && { echo "$out"; bad=1; }
done
[ $bad -eq 0 ] && echo "qmlcheck: all $(ls "$dir"/*.qml | wc -l) files parse"
exit $bad

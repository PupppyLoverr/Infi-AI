#!/bin/bash
# Live visual test: run the real solyra-comp (niri) nested in headless weston with software GL,
# the real panel and swaybg inside it, then screenshot via the compositor.
# usage: run.sh <name> <wallpaper.png> <shell.conf text> [wait] [extra cmd]
R=$(cd "$(dirname "$0")/.." && pwd)
L=${LIVE_DIR:-/tmp/solyra-live}; mkdir -p $L; grep -v "spawn-at-startup\|spawn-sh-at-startup" $R/base/solyra-config.kdl > $L/niri.kdl; name=$1; wp=$2; conf=$3; wait=${4:-8}; extra=$5
mkdir -p /tmp/sxr && chmod 700 /tmp/sxr; export XDG_RUNTIME_DIR=/tmp/sxr
H=$L/home-$name; rm -rf $H; mkdir -p $H/.config/solyra; printf "$conf" > $H/.config/solyra/shell.conf
weston --backend=${WESTON_BACKEND:-headless-backend.so} --socket=wl-$name --width=1280 --height=800 >$L/weston-$name.log 2>&1 &
WP=$!; sleep 2
export HOME=$H LIBGL_ALWAYS_SOFTWARE=1 WAYLAND_DISPLAY=wl-$name
$R/comp/target/debug/niri -c $L/niri.kdl >$L/niri-$name.log 2>&1 &
NP=$!; sleep 4
SOCK=$(grep -o "IPC listening on: [^ ]*" $L/niri-$name.log | tail -1 | awk '{print $NF}' | sed "s/\x1b\[[0-9;]*m//g")
NW=$(grep -o "listening on Wayland socket: [a-z0-9-]*" $L/niri-$name.log | awk '{print $NF}')
export NIRI_SOCKET=$SOCK WAYLAND_DISPLAY=$NW
swaybg -i $wp -m fill >/dev/null 2>&1 &
$R/shell/panel/target/debug/solyra-panel >$L/panel-$name.log 2>&1 &
[ -n "$extra" ] && bash -c "$extra" >/dev/null 2>&1 &
sleep $wait
rm -f $L/$name.png
$R/comp/target/debug/niri msg action screenshot-screen --path $L/$name.png 2>&1 | tail -2; sleep 3
[ -f $L/$name.png ] && echo "shot $L/$name.png" || echo "no shot (sock=$SOCK)"
pkill -P $$ 2>/dev/null; kill $NP $WP 2>/dev/null; wait 2>/dev/null

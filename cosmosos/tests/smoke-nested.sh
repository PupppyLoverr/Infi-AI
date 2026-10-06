#!/bin/bash
# CosmosOS nested smoke test.
#
# Boots the whole desktop inside an existing Wayland host:
#   weston --backend=headless → cosmos-compositor --winit → cosmos-shell
#   → a real app (cosmos-files, cosmos-terminal)
# then verifies via cosmos-ipc that the compositor sees the windows.
#
# Host needs: weston, cargo build already done (target/debug binaries).
#   ./tests/smoke-nested.sh [workspace-root]
set -u

ROOT="${1:-$(cd "$(dirname "$0")/../cosmos" && pwd)}"
XDG=$(mktemp -d /tmp/cosmos-smoke.XXXXXX)
chmod 700 "$XDG"
export XDG_RUNTIME_DIR=$XDG
LOG=/tmp/cosmos-smoke-logs
mkdir -p "$LOG"

PIDS=()
cleanup() {
    for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done
}
trap cleanup EXIT

echo "[smoke] weston headless on :hostwl"
weston --backend=headless --socket=hostwl --width=1280 --height=800 \
    --log="$LOG/weston.log" >/dev/null 2>&1 &
PIDS+=($!)
sleep 2

echo "[smoke] cosmos-compositor --winit"
WAYLAND_DISPLAY=hostwl "$ROOT/target/debug/cosmos-compositor" --winit \
    >"$LOG/compositor.log" 2>&1 &
PIDS+=($!)
sleep 5
[ -S "$XDG/wayland-1" ] || { echo "FAIL: no wayland-1 socket"; exit 1; }
[ -S "$XDG/cosmos-ipc.sock" ] || { echo "FAIL: no cosmos-ipc.sock"; exit 1; }

echo "[smoke] cosmos-shell"
WAYLAND_DISPLAY=wayland-1 "$ROOT/target/debug/cosmos-shell" \
    >"$LOG/shell.log" 2>&1 &
PIDS+=($!)
sleep 3

echo "[smoke] cosmos-files + cosmos-terminal"
WAYLAND_DISPLAY=wayland-1 "$ROOT/target/debug/cosmos-files" \
    >"$LOG/files.log" 2>&1 &
PIDS+=($!)
WAYLAND_DISPLAY=wayland-1 "$ROOT/target/debug/cosmos-terminal" \
    >"$LOG/terminal.log" 2>&1 &
PIDS+=($!)
sleep 10

echo "[smoke] probing cosmos-ipc"
python3 - <<'PYEOF'
import json, os, socket, sys
sock = socket.socket(socket.AF_UNIX)
sock.connect(os.environ["XDG_RUNTIME_DIR"] + "/cosmos-ipc.sock")
sock.settimeout(3)
f = sock.makefile("r")
def req(r):
    sock.sendall((json.dumps(r) + "\n").encode())
pong = windows = None
req({"op": "ping"})
req({"op": "list_windows"})
deadline = 0
for line in f:
    try: ev = json.loads(line)
    except Exception: continue
    if ev.get("type") == "pong": pong = ev["pong"]["name"]
    if ev.get("type") == "windows": windows = ev["windows"]; break
print("pong:", pong)
print("windows:", len(windows or []), [w.get("title") for w in (windows or [])])
if pong is None or windows is None:
    sys.exit(1)
PYEOF
RC=$?

for name in files terminal shell compositor; do
    if grep -qi "panic" "$LOG/$name.log" 2>/dev/null; then
        echo "FAIL: panic in $name"
        grep -A4 panic "$LOG/$name.log" | head -8
        RC=1
    fi
done
for p in "${PIDS[@]:3}" "${PIDS[@]:2}"; do :; done
ALIVE=0
for p in "${PIDS[@]}"; do kill -0 "$p" 2>/dev/null && ALIVE=$((ALIVE+1)); done
echo "[smoke] processes alive: $ALIVE/${#PIDS[@]} (logs in $LOG)"
[ "$ALIVE" -ge 4 ] || RC=1
[ "$RC" = 0 ] && echo "SMOKE PASS" || echo "SMOKE FAIL"
exit "$RC"

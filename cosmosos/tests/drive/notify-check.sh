#!/bin/sh
# notify-check.sh — guest-side verifier for notification action buttons,
# agentd approval cards (#113), default-sensitive gating (#115) and the
# cosmos-agents app (#114). Injected by inject-notify-check.sh.
#
# Marker flow: `say` writes go to ttyS0 -> host clicks card/banner buttons
# while socat calls block on the approval decision (up to ~125s each).

DIR=/var/lib/cosmos/notify
OUT=$DIR/evidence.txt
mkdir -p "$DIR"
exec >"$OUT" 2>&1

say() {
  echo "$@"
  echo "$@" > /dev/ttyS0 2>/dev/null || true
}

XRD=/run/user/1000
IPC=$XRD/cosmos-ipc.sock
SOCK=$XRD/cosmos-agentd.sock

say "==NOTIFY-CHECK=="
say "utc=$(date -u +%FT%TZ)"

i=0
while [ ! -S "$IPC" ] && [ $i -lt 240 ]; do sleep 1; i=$((i+1)); done
[ -S "$IPC" ] && say "PASS: session up (${i}s)" || say "FAIL: no session"
say "==NOTIFY-READY=="

MONLOG=$DIR/busmon.log
timeout 900 su -l cosmos -c "XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus busctl --user monitor org.freedesktop.Notifications" > "$MONLOG" 2>&1 &

say "==NOTIFY-ACTIONS=="   # host: dump banner, click a pill (~35s window)
timeout 10 su -l cosmos -c "gdbus call --session -d org.freedesktop.Notifications -o /org/freedesktop/Notifications -m org.freedesktop.Notifications.Notify 'drivetest' 0 '' 'Action Test' 'pick a button' '[\"open\",\"Open\",\"dismiss\",\"Dismiss\"]' '{}' 35000" >/dev/null 2>&1
sleep 40
grep -q ActionInvoked "$MONLOG" \
  && say "PASS: ActionInvoked round-trip" \
  || say "FAIL: no ActionInvoked in bus monitor"

say "==NOTIFY-PLAIN=="     # plain banner — render/expire unchanged
timeout 10 su -l cosmos -c "gdbus call --session -d org.freedesktop.Notifications -o /org/freedesktop/Notifications -m org.freedesktop.Notifications.Notify 'drivetest' 0 '' 'Plain Test' 'no actions' '[]' '{}' 4000" >/dev/null 2>&1
sleep 6

mc() {
  printf '%s\n%s\n' "$3" "$2" \
    | timeout "$1" su -l cosmos -c "socat -t $4 - UNIX-CONNECT:$SOCK" 2>&1
}
INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test-agent","version":"0.1"},"protocolVersion":"2024-11-05","capabilities":{}}}'

say "==APPROVAL-ALLOW=="   # host: dump card, click 'Allow once'
RESP=$(mc 130 '{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"files.write","arguments":{"path":"/home/cosmos/Agents/test-agent/ok.txt","content":"x"}}}' "$INIT" 125)
echo "$RESP" | sed 's/^/  mcp: /'
{ echo "$RESP" | grep -qi 'wrote\|ok\|success'; } \
  && say "PASS: Allow-once write proceeded" \
  || say "FAIL: allow-once -> $RESP"

say "==APPROVAL-DENY=="    # host: click 'Deny'
RESP=$(mc 130 '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"files.write","arguments":{"path":"/home/cosmos/Agents/test-agent/deny.txt","content":"n"}}}' "$INIT" 125)
echo "$RESP" | sed 's/^/  mcp: /'
{ echo "$RESP" | grep -qi 'denied\|error' && [ ! -f /home/cosmos/Agents/test-agent/deny.txt ]; } \
  && say "PASS: Deny returned error, no file" \
  || say "FAIL: deny -> $RESP"

say "==APPROVAL-ALWAYS=="  # host: click 'Always allow'
RESP=$(mc 130 '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"files.write","arguments":{"path":"/home/cosmos/Agents/test-agent/always.txt","content":"a"}}}' "$INIT" 125)
echo "$RESP" | grep -qi 'wrote\|ok\|success' \
  && say "PASS: Always write proceeded" \
  || say "FAIL: always -> $RESP"
say "==APPROVAL-REPEAT=="  # same tool again — card should NOT appear
RESP=$(mc 30 '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"files.write","arguments":{"path":"/home/cosmos/Agents/test-agent/always2.txt","content":"b"}}}' "$INIT" 25)
echo "$RESP" | grep -qi 'wrote\|ok\|success' \
  && say "PASS: repeat call skipped the card" \
  || say "FAIL: repeat still gated -> $RESP"

# sensitive=[] opt-out (PR #115): explicit empty list bypasses defaults
timeout 10 su -l cosmos -c 'mkdir -p /home/cosmos/.config/cosmos/agents /home/cosmos/Agents/free-agent; printf "sensitive = []\n" > /home/cosmos/.config/cosmos/agents/free-agent.toml'
say "==SENSITIVE-EMPTY=="  # no card expected — direct write
INIT2='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"free-agent","version":"0.1"},"protocolVersion":"2024-11-05","capabilities":{}}}'
RESP=$(mc 30 '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"files.write","arguments":{"path":"/home/cosmos/Agents/free-agent/free.txt","content":"f"}}}' "$INIT2" 25)
echo "$RESP" | grep -qi 'wrote\|ok\|success' \
  && say "PASS: sensitive=[] bypassed the card" \
  || say "FAIL: sensitive=[] -> $RESP"

# Focus mode: critical card must still show while banners are hidden.
# Host enables Focus via quick settings during the ==FOCUS-ON== window,
# then we fire a plain banner (should be suppressed) and an approval
# (must still appear). Assertions live in the host dumps.
say "==FOCUS-ON=="
sleep 20
timeout 10 su -l cosmos -c "gdbus call --session -d org.freedesktop.Notifications -o /org/freedesktop/Notifications -m org.freedesktop.Notifications.Notify 'drivetest' 0 '' 'Focus Test' 'should be hidden' '[]' '{}' 8000" >/dev/null 2>&1
sleep 4
say "==FOCUS-APPROVAL=="   # host: dump — critical card expected visible
RESP=$(mc 130 '{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"desktop.screenshot","arguments":{}}}' "$INIT" 125)
echo "$RESP" | sed 's/^/  mcp: /'
say "==FOCUS-OFF=="        # host: disable Focus again
sleep 20

# cosmos-agents app (#114): launch it — policy list + audit feed should
# show test-agent's calls; host screendumps the UI.
say "==AGENTS-APP=="
timeout 10 su -l cosmos -c "XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 DISPLAY=:0 cosmos-agents >/tmp/cosmos-agents.log 2>&1 &" 2>/dev/null
sleep 12
timeout 5 su -l cosmos -c 'cat /tmp/cosmos-agents.log 2>/dev/null' | sed 's/^/  agents: /'
pgrep -f cosmos-agents >/dev/null && say "PASS: cosmos-agents running" || say "FAIL: cosmos-agents not running"
sleep 15

AUDIT=$(timeout 10 su -l cosmos -c 'cat ~/.local/share/cosmos/agent-audit/day-*.jsonl 2>/dev/null' | wc -l)
say "agent-audit lines: $AUDIT"
timeout 10 su -l cosmos -c 'tail -8 ~/.local/share/cosmos/agent-audit/day-*.jsonl' 2>/dev/null | sed 's/^/  audit: /'

PANICS=$(journalctl -b --no-pager 2>/dev/null | grep -ci 'panic\|core dumped\|segfault' || true)
say "panic count this boot: $PANICS"
[ "$PANICS" = 0 ] && say "PASS: zero panics" || say "FAIL: $PANICS panics in journal"
journalctl -b --no-pager 2>/dev/null | grep -i 'greetd\|check_children\|respawn\|restart' | tail -12 | sed 's/^/  greetd: /'

say "==NOTIFY-END=="
sync; sleep 2
cat "$OUT" > /dev/ttyS0 2>/dev/null || true

#!/bin/sh
# lock-check.sh — guest-side verifier for the greetd + session-lock
# stack. Installed by inject-lock-check.sh, run once at boot.
#
# The real greeter/lock interactions are driven host-side via QMP
# sendkey + screendumps (tests/drive/lock.sh). This script:
#   1. waits for the cosmos session (ipc sock) — that alone proves
#      greetd -> cosmos-greeter -> StartSession -> cosmos-session worked
#   2. runs the cosmos-agentd MCP smoke (socket, handshake, tools/list,
#      files.write policy deny/allow, audit journal)
#   3. sweeps for compositor panics and emits evidence to ttyS0.
#
# Marker flow: `say` writes go to ttyS0 -> host acts on them.

DIR=/var/lib/cosmos/lock
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

say "==LOCK-CHECK=="
say "utc=$(date -u +%FT%TZ)"

# Stage 1 — session must come up through greetd. Poll the compositor
# ipc socket; its existence means a StartSession succeeded.
i=0
while [ ! -S "$IPC" ] && [ $i -lt 240 ]; do sleep 1; i=$((i+1)); done
if [ -S "$IPC" ]; then
  say "PASS: cosmos session up through greetd (ipc sock after ${i}s)"
else
  say "FAIL: no cosmos-ipc.sock after 240s — greeter login never produced a session"
fi
say "==LOCK-READY=="

# Stage 2 — cosmos-agentd MCP smoke (owner's checklist, verbatim).
say "==AGENTD=="
if [ -S "$SOCK" ]; then
  say "PASS: $SOCK present"
  ls -la "$SOCK" | sed 's/^/  /'
else
  say "FAIL: $SOCK missing after session start"
fi

if command -v socat >/dev/null && [ -S "$SOCK" ]; then
  REQ='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test-agent","version":"0.1"},"protocolVersion":"2024-11-05","capabilities":{}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"desktop.windows.list","arguments":{}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"files.write","arguments":{"path":"/home/cosmos/Documents/evil.txt","content":"x"}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"files.write","arguments":{"path":"/home/cosmos/Agents/test-agent/ok.txt","content":"x"}}}'
  RESP=$(printf '%s\n' "$REQ" | timeout 15 su -l cosmos -c "socat -t 10 - UNIX-CONNECT:$SOCK" 2>&1)
  echo "$RESP" | sed 's/^/  mcp: /'

  echo "$RESP" | grep -q '"serverInfo".*cosmos-agentd' \
    && say "PASS: initialize -> serverInfo cosmos-agentd" \
    || say "FAIL: no serverInfo in initialize reply"
  NTOOLS=$(echo "$RESP" | grep '"tools"' | grep -o '"name": *"[a-z_.]*"' | wc -l)
  say "tools/list returned $NTOOLS tools (expect 15)"
  [ "$NTOOLS" = 15 ] && say "PASS: 15 tools" || say "FAIL: tool count $NTOOLS != 15"
  echo "$RESP" | grep -qi '"id":3' \
    && say "PASS: desktop.windows.list answered" \
    || say "FAIL: windows.list unanswered"
  echo "$RESP" | grep '"id":4' | grep -qi 'outside\|error\|denied' \
    && say "PASS: files.write outside write roots denied" \
    || say "FAIL: evil write not denied"
  [ -f /home/cosmos/Documents/evil.txt ] \
    && say "FAIL: evil.txt exists on disk" \
    || say "PASS: no evil.txt on disk"
  [ -f /home/cosmos/Agents/test-agent/ok.txt ] \
    && say "PASS: ok.txt written inside write root" \
    || say "FAIL: ok.txt missing"
  sleep 1
  AUDIT=$(timeout 10 su -l cosmos -c 'cat ~/.local/share/cosmos/agent-audit/day-*.jsonl 2>/dev/null' | wc -l)
  say "agent-audit lines: $AUDIT (expect >=2: ok + denied)"
  timeout 10 su -l cosmos -c 'tail -5 ~/.local/share/cosmos/agent-audit/day-*.jsonl' 2>/dev/null | sed 's/^/  audit: /'
fi

say "==AGENTD-DONE=="

# Stage 3 — keep the service alive while the host drives lock/unlock
# flows; then sweep the journal for compositor crashes.
i=0
while [ $i -lt 300 ]; do
  sleep 20
  [ -S "$IPC" ] || say "WARN: ipc sock vanished mid-drive"
  i=$((i+20))
done

PANICS=$(journalctl -b --no-pager 2>/dev/null | grep -ci 'panic\|core dumped\|segfault' || true)
say "compositor/session panic count this boot: $PANICS"
[ "$PANICS" = 0 ] && say "PASS: zero panics" || say "FAIL: $PANICS panics in journal"
journalctl -b --no-pager 2>/dev/null | grep -i 'panic\|core dumped\|segfault' | tail -10 | sed 's/^/  panic: /'
uptime | sed 's/^/  /'

say "==LOCK-END=="
sync; sleep 2
# Drive-harvest trap: cat the evidence to serial too — the disk copy can
# lag if the guest is killed without umount.
cat "$OUT" > /dev/ttyS0 2>/dev/null || true

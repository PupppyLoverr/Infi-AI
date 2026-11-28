#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/install-krun-agent.sh and scripts/krun-vm.sh, as they are, without launchd, krunkit, gvproxy or a guest:
#  - launchctl is a small model of launchd for one agent (bootstrap, bootout, print): it runs the agent's program with
#    the plist's environment, starts it again at once when it exits (KeepAlive, for a job that has run longer than its
#    ThrottleInterval, as a VM that is being stopped has), and on bootout sends it SIGTERM and ends what is left of it
#  - krunkit is a "VM" that writes what happens to it to $T/vm.log: "N start", "N up" once its guest has run for 2 s,
#    "N off" when the guest powered off (ssh ... 'sudo systemctl poweroff', 0.3 s later), "N killed [with its guest up]"
#  - gvproxy makes its socket; curl answers for its API as a real gvproxy does (leases as {ip: mac}; a forward of a
#    taken port: HTTP 500 with the reason, which is curl's status 22)
. "$(dirname "$0")/lib.sh"
sandbox_repo
K=$HOME/.local/krunkit; S=$HOME/.local/state/xnupoc-krun; D=$HOME/.tart/vms/xnupoc
PL=$HOME/Library/LaunchAgents/org.xnulinux.krun-vm.plist
mkdir -p "$K/bin" "$D" "$T/launchd"; : > "$D/disk.img"; echo '{"macAddress": "52:54:00:12:34:56"}' > "$D/config.json"
: > "$T/vm.log"; : > "$T/vmkey"	# the key of the Mac-side scripts (scripts/vm.sh), next to the repository
cat > "$T/launchd.py" <<'EOF'
import os, plistlib, signal, subprocess, sys, threading, time
T = os.environ['T']; cmd, arg = sys.argv[1], sys.argv[2]
pidfile = lambda label: f'{T}/launchd/{label}.pid'
def loaded(label):
    try: pid = int(open(pidfile(label)).read()); os.kill(pid, 0); return pid
    except (OSError, ValueError): return 0
if cmd == 'print':      # a domain (gui/UID), or a service in it (gui/UID/LABEL)
    sys.exit(0 if arg.count('/') == 1 or loaded(arg.split('/')[2]) else 113)
if cmd == 'bootout':
    pid = loaded(arg.split('/')[2])
    if not pid: sys.exit('Boot-out failed: 3: No such process')
    os.kill(pid, signal.SIGTERM)
    while loaded(arg.split('/')[2]): time.sleep(0.05)
    sys.exit(0)
assert cmd == 'bootstrap'
pl = plistlib.load(open(sys.argv[3], 'rb'))
if loaded(pl['Label']): sys.exit('Bootstrap failed: 5: Input/output error')
if os.fork(): sys.exit(0)
os.setsid(); open(pidfile(pl['Label']), 'w').write(str(os.getpid()))
env = dict(pl.get('EnvironmentVariables', {}), HOME=os.environ['HOME'])
env['PATH'] = f'{T}/bin:' + env.get('PATH', '/usr/bin:/bin')        # the stand-ins (curl) before the agent's PATH
log = open(pl['StandardOutPath'], 'ab'); job = None; stopping = False
def kill_group(pg):
    try: os.killpg(pg, signal.SIGKILL)
    except OSError: pass
def term(*_):
    global stopping; stopping = True
    if job and job.poll() is None: job.terminate()
signal.signal(signal.SIGTERM, term)
while not stopping:
    job = subprocess.Popen(pl['ProgramArguments'], env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    if stopping: job.terminate()
    job.wait()
    threading.Timer(0.5, kill_group, [job.pid]).start()         # what the job left running goes with it
    if not pl.get('KeepAlive'): break
os.unlink(pidfile(pl['Label']))
EOF
stub launchctl <<'EOF'
exec python3 "$T/launchd.py" "$@"
EOF
cat > "$T/krunkit.py" <<'EOF'
import os, signal, sys, time
T, VM = os.environ['T'], os.environ['VM']; t0 = time.time(); said = False
n = sum(l.endswith(' start\n') for l in open(f'{T}/vm.log')) + 1
def log(what): open(f'{T}/vm.log', 'a').write(f'{n} {what}\n')
def up(): return time.time() - t0 >= 2
def term(*_): log('killed with its guest up' if up() else 'killed'); os._exit(143)
signal.signal(signal.SIGTERM, term); log('start')
while not os.path.exists(f'{VM}/poweroff'):
    if up() and not said: said = True; log('up')
    time.sleep(0.05)
time.sleep(0.3); os.unlink(f'{VM}/poweroff'); log('off')
EOF
stub krunkit "$K/bin" <<'EOF'
exec python3 "$T/krunkit.py" "$0" "$@"	# one process, with krunkit's path and arguments in its command line
EOF
stub gvproxy "$K/bin" <<'EOF'
while [ $# -gt 0 ]; do [ "$1" != -listen-vfkit ] || s=${2#unixgram://}; shift; done
cd "$(dirname "$s")" && exec python3 -c 'import socket, sys, time; socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM).bind(sys.argv[1]); time.sleep(1000000)' "$(basename "$s")" "$s"
EOF
stub curl <<'EOF'
case "$*" in
  */services/dhcp/leases*) echo '{"192.168.127.1":"5a:94:ef:e4:0c:dd","192.168.127.2":"52:54:00:12:34:56"}';;
  */services/forwarder/expose*) [ ! -e "$T/port-taken" ] || { case "$*" in *--fail-with-body*) echo 'listen tcp 127.0.0.1:2223: bind: address already in use';; esac; exit 22; };;
esac
EOF
stub_ssh
vmstub sudo <<'EOF'
exec "$@"
EOF
vmstub systemctl <<'EOF'
[ "$1" != poweroff ] || : > "$VM/poweroff"
EOF
A=$T/repo/scripts/install-krun-agent.sh
vm() { grep -qx "$1" "$T/vm.log"; }
krunkits() { pgrep -f "$K/bin/krunkit" >/dev/null; }

/bin/bash "$A" > "$T/out" 2>&1
check 'install: launchd runs krun-vm.sh, the VM starts and its ssh is forwarded' wait_for 10 grep -qx 'ssh: 127.0.0.1:2223 -> 192.168.127.2:22' "$S/agent.log"
wait_for 10 vm '1 up'
within 20 /bin/bash "$A" remove >> "$T/out" 2>&1; s=$?
check 'remove: the guest is asked to power off (ssh to the SSH_PORT of the agent) and its VM ends by that' eval 'called "^ssh .* -p 2223 .*systemctl poweroff" && vm "1 off"'
check "... with the key of the Mac-side scripts (../vmkey, as scripts/vm.sh)" called "^ssh .* -i $T/repo/\.\./vmkey .*systemctl poweroff"
check '... remove returns when that VM is down, not after waiting out the one launchd started in its place' [ $s = 0 ]
check '... leaving no VM running, and none killed with its guest up' not eval 'krunkits || grep -q "guest up" "$T/vm.log"'
check '... and no agent' not eval '[ -e "$PL" ] || launchctl print gui/$(id -u)/org.xnulinux.krun-vm'
[ $FAILED = 0 ] || { sed 's/^/    | /' "$T/out" "$T/vm.log"; }
pkill -f "$T/launchd.py" 2>/dev/null; pkill -f "$K/bin/" 2>/dev/null; sleep 0.3	# (what a failed remove left)

: > "$T/vm.log"; : > "$T/calls"; : > "$S/agent.log"; f=$FAILED
/bin/bash "$A" >> "$T/out" 2>&1; wait_for 10 vm '1 up'
CPUS=4 within 20 /bin/bash "$A" >> "$T/out" 2>&1; s=$?
check 'install over a running agent (new settings): its guest is powered off first, not killed' eval '[ $s = 0 ] && vm "1 off" && ! grep -q "guest up" "$T/vm.log"'
check '... and the VM comes back with the new settings' wait_for 10 eval 'grep -q "^krunkit --cpus 4 " "$T/calls" && pgrep -f "$K/bin/krunkit --cpus 4 " >/dev/null'
[ $FAILED = $f ] || { sed 's/^/    | /' "$T/out" "$T/vm.log"; }
launchctl bootout "gui/$(id -u)/org.xnulinux.krun-vm" 2>/dev/null; pkill -f "$K/bin/" 2>/dev/null; sleep 0.3

: > "$T/port-taken"; : > "$T/calls"; f=$FAILED
/bin/bash "$T/repo/scripts/krun-vm.sh" > "$T/vm.out" 2>&1 & pid=$!
check 'krun-vm.sh, SSH_PORT taken: it says that the VM has no ssh, and why' wait_for 10 grep -q 'no ssh.*127.0.0.1:2223.*address already in use' "$T/vm.out"
check '... and runs the VM all the same' wait_for 5 called '^krunkit '
kill $pid 2>/dev/null; wait $pid 2>/dev/null
[ $FAILED = $f ] || sed 's/^/    | /' "$T/vm.out"
finish

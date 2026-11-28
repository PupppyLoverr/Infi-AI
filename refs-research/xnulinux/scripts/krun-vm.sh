#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On the Mac host: run the xnupoc Linux VM under krunkit (libkrun on Hypervisor.framework) instead of tart, for a GPU the
# guest can use: krunkit gives Linux a virtio-gpu with Venus (Vulkan commands forwarded to virglrenderer, then MoltenVK and
# Metal on the host GPU). The VM's disk is tart's own, and two VMs on one disk destroy its file system: krunkit runs holding
# tart's own lock on the VM (an fcntl write lock on its config.json), so `tart list` shows it running, `tart run` refuses
# to start it (and `tart stop` ends krunkit), and this script refuses while tart, or another krunkit, has it. Networking is gvproxy
# (user-mode, NAT to the host's network) with the guest's SSH on 127.0.0.1:$SSH_PORT, forwarded to whatever address the
# guest got from gvproxy's DHCP (it is not always .2: a lease from an earlier run can move it to .3).
#
# Needs krunkit, libkrun-efi, virglrenderer, MoltenVK and gvproxy under $KRUN (scripts/krun-install.sh).
set -euo pipefail
KRUN=${KRUN:-$HOME/.local/krunkit}
DISK=${DISK:-$HOME/.tart/vms/xnupoc/disk.img}
MAC=${MAC:-$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["macAddress"])' "$(dirname "$DISK")/config.json")}
CPUS=${CPUS:-6}; MEM=${MEM:-16384}; SSH_PORT=${SSH_PORT:-2223}
S=${S:-$HOME/.local/state/xnupoc-krun}; mkdir -p "$S"
CONFIG=$(dirname "$DISK")/config.json
# locked COMMAND...: takes tart's lock on the VM, then becomes COMMAND, which keeps it for its lifetime (an fcntl lock is the
# process's own, so exec keeps it; the descriptor must stay open)
locked() {
  exec python3 - "$CONFIG" "$@" <<'PY'
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_RDWR)
try:
    fcntl.lockf(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
except OSError:
    sys.exit(f'{os.path.dirname(sys.argv[1])}: the VM is running (tart or another krunkit has its lock): stop it first')
os.set_inheritable(fd, True)
os.execv(sys.argv[2], sys.argv[2:])
PY
}
(locked /usr/bin/true) || exit 1	# before anything of a running VM's is touched
rm -f "$S"/gv.sock "$S"/gvapi.sock
"$KRUN"/bin/gvproxy -listen-vfkit "unixgram://$S/gv.sock" -listen "unix://$S/gvapi.sock" -ssh-port -1 \
  -log-file "$S/gvproxy.log" -pid-file "$S/gvproxy.pid" &
GV=$!
trap 'kill $GV 2>/dev/null' EXIT
for _ in $(seq 1 50); do [ -S "$S/gv.sock" ] && break; sleep 0.1; done
: > "$S/console.log"
locked "$KRUN"/bin/krunkit --cpus "$CPUS" --memory "$MEM" \
  --bootloader "efi,variable-store=$S/efi-vars,create" \
  --device "virtio-blk,path=$DISK,format=raw" \
  --device "virtio-net,type=unixgram,path=$S/gv.sock,mac=$MAC,offloading=on,vfkitMagic=on" \
  --device "virtio-serial,logFilePath=$S/console.log" \
  --device "virtio-gpu,width=1280,height=800" \
  --restful-uri "unix://$S/rest.sock" --pidfile "$S/krunkit.pid" --log-file "$S/krunkit.log" &
KR=$!
trap 'kill $KR $GV 2>/dev/null' EXIT
api() { curl -s --fail-with-body --unix-socket "$S/gvapi.sock" "http://gv$1" "${@:2}"; }	# an HTTP error: status 22 and gvproxy's reason
for _ in $(seq 1 120); do
  ip=$(api /services/dhcp/leases | python3 -c 'import json,sys; m=sys.argv[1].lower(); print(next((k for k,v in json.load(sys.stdin).items() if v.lower()==m), ""))' "$MAC" 2>/dev/null || true)
  [ -n "$ip" ] && break; sleep 1
done
if [ -n "$ip" ]; then
  # gvproxy says why not (SSH_PORT taken: "listen tcp ...: bind: address already in use"); the VM runs on, without ssh
  why=$(api /services/forwarder/expose -X POST -d "{\"local\":\"127.0.0.1:$SSH_PORT\",\"remote\":\"$ip:22\"}") &&
    echo "ssh: 127.0.0.1:$SSH_PORT -> $ip:22" || echo "no ssh: gvproxy did not forward 127.0.0.1:$SSH_PORT to the guest: ${why:-no answer}" >&2
else
  echo "the guest never took a DHCP lease" >&2
fi
wait $KR

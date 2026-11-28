#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM (once, after scripts/vm-install-fki-service.sh and with the disk from scripts/mk-macvm-disk.sh): run the hosted
# macOS as a system service. It starts at boot, is stopped cleanly (its disk synced) when the VM shuts down, and is
# started again after a panic or a run that ended by itself; each run's kernel log is in /var/log/xnulinux/.
#   sudo systemctl status xnulinux     journalctl -u xnulinux     settings: /etc/xnulinux.conf (then: systemctl restart xnulinux)
#   scripts/install-service.sh remove    stop the hosted system (its disk synced first) and take the service away again
set -euo pipefail
P=$(cd "$(dirname "$0")/.." && pwd)
if [ "${1:-}" = remove ]; then
  # stopped through the unit while there is one (its ExecStop is xnulinux-boot.sh stop); a system started by hand the same way
  if [ -f /etc/systemd/system/xnulinux.service ]; then sudo systemctl disable --now xnulinux.service; fi
  [ ! -d /sys/module/xnu ] || (cd "$P" && scripts/xnulinux-boot.sh stop)
  sudo rm -f /etc/systemd/system/xnulinux.service
  sudo systemctl daemon-reload
  echo "removed: xnulinux.service (kept: /etc/xnulinux.conf, the logs in /var/log/xnulinux, the disk; fki.ko and the memory set aside for it: scripts/vm-install-fki-service.sh remove)"
  exit 0
fi
U=$(id -un)
# The service runs xnulinux-boot.sh as this user with no terminal: every sudo in it has to work without a password
# (-k: not on the strength of one typed a moment ago). Said here, not by a unit that fails at the next boot.
[ "$(id -u)" = 0 ] || sudo -k -n true 2>/dev/null || {
  echo "$U cannot use sudo without a password, and the service runs scripts/xnulinux-boot.sh as $U with no terminal to ask on:" >&2
  echo "give $U a NOPASSWD rule (sudo visudo -f /etc/sudoers.d/xnulinux: '$U ALL=(ALL) NOPASSWD:ALL') or run this as root" >&2
  exit 1
}
# the disk's UUIDs, where the service's xnulinux-boot.sh will look for them: MAC_CONF of /etc/xnulinux.conf (a PureDarwin
# disk's pd.conf, say), /var/tmp/mac.conf without one
M=$(unset MAC_CONF; [ -f /etc/xnulinux.conf ] && . /etc/xnulinux.conf >/dev/null; echo "${MAC_CONF:-/var/tmp/mac.conf}")
(cd "$P" && [ -f "$M" ]) || { echo "no $M: make the disk first (scripts/mk-macvm-disk.sh, scripts/mk-puredarwin-disk.sh)" >&2; exit 1; }
[ -f /etc/xnulinux.conf ] || sudo tee /etc/xnulinux.conf >/dev/null <<'CONF'
# The hosted macOS (scripts/xnulinux-boot.sh reads this, then /var/tmp/mac.conf for the disk's UUIDs); the environment
# overrides what is written as ${NAME:-default}
CPUS=${CPUS:-2}		# its CPUs (the Linux CPUs from 0 up; Linux keeps the rest and its device interrupts)
RAM_MB=${RAM_MB:-7900}	# its memory, within fki's carve-out
#DISK=/var/tmp/mac.img
GPU=${GPU:-auto}	# auto: the host GPU when the VM has a render node; off: CPU rendering only
#XNU_ARGS="model=VirtualMac2,1"
#BOOTARGS="amfi_get_out_of_my_way=1 lp-sip0=2050 bt-transport-override=10"
CONF
# Wants=, not Requires=, the unit that loads fki.ko at boot: after a kernel update that load fails (fki.ko is still the old
# kernel's), and a required unit that failed keeps this one from ever starting. xnulinux-boot.sh builds both modules for
# the running kernel and loads fki.ko itself when it is not loaded, with that unit's arguments.
sudo tee /etc/systemd/system/xnulinux.service >/dev/null <<UNIT
[Unit]
Description=xnulinux: the hosted macOS
After=xnulinux-fki.service network-online.target
Wants=xnulinux-fki.service network-online.target
StartLimitIntervalSec=600
StartLimitBurst=5

[Service]
Type=simple
User=$U
WorkingDirectory=$P
ExecStart=$P/scripts/xnulinux-boot.sh --wait
ExecStop=$P/scripts/xnulinux-boot.sh stop
TimeoutStartSec=infinity
TimeoutStopSec=180
Restart=on-failure
RestartSec=10

[Install]
WantedBy=multi-user.target
UNIT
sudo systemctl daemon-reload
sudo systemctl enable xnulinux.service
echo "installed: sudo systemctl start xnulinux (and at every boot); settings in /etc/xnulinux.conf"

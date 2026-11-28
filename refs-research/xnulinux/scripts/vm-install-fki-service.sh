#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM (once): the hosted kernel's DRAM as RAM Linux never manages, and fki.ko loaded with it early in every boot.
# The VM has 16 GB (RAM from 0x40000000 to 0x440000000); Linux's command line says mem=7G (RAM up to 0x200000000), and the
# 8 GB above it are fki's carve-out (carve_base/carve_mb), ending ~1 GB below the top of RAM, where the firmware keeps its
# regions and loads the Linux kernel image (nokaslr: see below). A CMA area was not enough: one page Linux pins in it
# during the boot (a buffer-head page of the block device) splits it, and the 8 GB block does not fit.
# A VM with more memory: LINUX_MEM=15G CARVE_BASE=0x400000000 for 24 GB.
#   scripts/vm-install-fki-service.sh remove    give the memory back: the command line and the unit as they were before (after
#                                               scripts/install-service.sh remove; Linux has all of its RAM again after a reboot)
set -euo pipefail
P=$(cd "$(dirname "$0")/.." && pwd)
LINUX_MEM=${LINUX_MEM:-7G}; CARVE_BASE=${CARVE_BASE:-0x200000000}; CARVE_MB=${CARVE_MB:-8156}
G=/etc/default/grub.d/99-xnulinux.cfg
if [ "${1:-}" = remove ]; then
  [ ! -d /sys/module/xnu ] || { echo "a hosted system is running: scripts/install-service.sh remove (or scripts/xnulinux-boot.sh stop) first" >&2; exit 1; }
  if [ -f /etc/systemd/system/xnulinux-fki.service ]; then sudo systemctl disable --now xnulinux-fki.service; fi
  sudo rm -f /etc/systemd/system/xnulinux-fki.service "$G"
  sudo systemctl daemon-reload
  sudo update-grub >/dev/null 2>&1
  if [ -d /sys/module/fki ]; then sudo systemctl stop xnudrmd 2>/dev/null || true; sudo rmmod fki; fi	# (xnudrmd holds its /dev/xnu-hostcall)
  echo "removed: xnulinux-fki.service and the mem= command line ($G); fki.ko is unloaded, and after a reboot Linux has all of the machine's memory again"
  exit 0
fi
# the unit loads fki-ko/fki.ko at every boot, before anything else builds it (xnulinux-boot.sh, which runs from
# xnulinux.service after this unit): build it now
make -s -C /lib/modules/$(uname -r)/build M="$P/fki-ko" modules
# nokaslr: arm64's EFI stub randomises the Linux kernel's physical load address across all EFI memory before mem=
# takes effect, which otherwise places the kernel image inside the carve-out on some boots (fki then refuses to map it).
echo "GRUB_CMDLINE_LINUX=\"\$GRUB_CMDLINE_LINUX mem=$LINUX_MEM nokaslr log_buf_len=128M\"" | sudo tee $G >/dev/null
sudo update-grub >/dev/null 2>&1
sudo tee /etc/systemd/system/xnulinux-fki.service >/dev/null <<UNIT
[Unit]
Description=xnulinux: fki.ko with the hosted kernel's DRAM reserved
DefaultDependencies=no
After=local-fs.target
Before=sysinit.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/sbin/insmod $P/fki-ko/fki.ko carve_base=$CARVE_BASE carve_mb=$CARVE_MB

[Install]
WantedBy=sysinit.target
UNIT
sudo systemctl daemon-reload
sudo systemctl enable xnulinux-fki.service
sync
echo "after a reboot: Linux on $LINUX_MEM, fki.ko loaded with the $CARVE_MB MB carve-out at $CARVE_BASE (rebuild fki-ko before a reboot when it changes)"

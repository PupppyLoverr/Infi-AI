#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# `scripts/install-service.sh remove` and `scripts/vm-install-fki-service.sh remove` on a stand-in host. The scripts are
# the repository's, with one change made here: /etc and /sys/module start under a temporary directory. sudo runs its
# command; systemctl, update-grub and rmmod are stand-ins that write down how they were called (rmmod also "unloads").
. "$(dirname "$0")/lib.sh"
sandbox_repo
P=$T/repo; R=$T/root; export R
for s in install-service.sh vm-install-fki-service.sh; do
  sed -e "s|/sys/module|$R/sys/module|g" -e "s|/etc/|$R/etc/|g" "$REPO/scripts/$s" > "$P/scripts/$s"
done
cat > "$P/scripts/xnulinux-boot.sh" <<'EOF'
#!/bin/bash
echo "xnulinux-boot.sh $*" >> "$T/calls"; [ "$1" != stop ] || rm -rf "$R/sys/module/xnu"
EOF
chmod +x "$P/scripts/xnulinux-boot.sh"; export T
stub sudo <<'EOF'
while [ $# -gt 0 ]; do case $1 in -n|-E|-k) shift;; *=*) export "$1"; shift;; *) break;; esac; done
exec "$@"
EOF
stub systemctl <<'EOF'
case "$1 $2" in "disable --now") [ "$3" != xnulinux.service ] || rm -rf "$R/sys/module/xnu";; esac	# its ExecStop ends the system
EOF
stub update-grub < /dev/null
stub make <<'EOF'
echo "$*" >> "$T/make"
EOF
stub rmmod <<'EOF'
[ -d "$R/sys/module/$1" ] || { echo "rmmod: ERROR: Module $1 is not currently loaded" >&2; exit 1; }
rm -rf "$R/sys/module/$1"
EOF
U=$R/etc/systemd/system; G=$R/etc/default/grub.d/99-xnulinux.cfg
host() {	# a host with both installed and a hosted system running ($1 = idle: none running)
  rm -rf "$R"; mkdir -p "$U" "$R/etc/default/grub.d" "$R/sys/module/fki"; : > "$T/calls"
  echo unit > "$U/xnulinux.service"; echo unit > "$U/xnulinux-fki.service"; echo 'GRUB_CMDLINE_LINUX="mem=7G"' > "$G"; echo conf > "$R/etc/xnulinux.conf"
  [ "${1:-}" = idle ] || mkdir -p "$R/sys/module/xnu"
}
run() { /bin/bash "$P/scripts/$1" remove > "$T/out" 2>&1; }

host; run vm-install-fki-service.sh; s=$?
check 'vm-install-fki-service.sh remove with a hosted system running: refused, nothing touched' \
  [ $s = 1 -a -e "$U/xnulinux-fki.service" -a -e "$G" -a -d "$R/sys/module/fki" ]
check '... and it names what to run first' grep -q 'install-service.sh remove' "$T/out"

run install-service.sh; s=$?
check 'install-service.sh remove: status 0, the unit gone' [ $s = 0 -a ! -e "$U/xnulinux.service" ]
check '... stopped through the unit (disable --now), which ends the hosted system' called '^systemctl disable --now xnulinux.service'
check '... systemd told (daemon-reload)' called '^systemctl daemon-reload'
check "... the settings and fki's unit left alone" [ -e "$R/etc/xnulinux.conf" -a -e "$U/xnulinux-fki.service" -a -e "$G" ]
check '... and it says what it kept and names the other remove' grep -q 'kept: .*vm-install-fki-service.sh remove' "$T/out"

run vm-install-fki-service.sh; s=$?
check 'vm-install-fki-service.sh remove: status 0, the unit and the mem= command line gone' [ $s = 0 -a ! -e "$U/xnulinux-fki.service" -a ! -e "$G" ]
check '... the boot loader told (update-grub)' called '^update-grub'
check '... fki.ko unloaded' [ ! -d "$R/sys/module/fki" ]
check '... and it says a reboot gives the memory back' grep -q 'after a reboot' "$T/out"

: > "$T/calls"; run install-service.sh; a=$?; run vm-install-fki-service.sh; b=$?
check 'both again on the host that has nothing of theirs left: status 0' [ $a = 0 -a $b = 0 ]
check '... without stopping or unloading anything' not called '^systemctl disable|^rmmod'

# a hosted system started by hand, with no unit: install-service.sh remove still ends it
host; rm "$U/xnulinux.service"; run install-service.sh; s=$?
check 'install-service.sh remove, a system running without the unit: ended through xnulinux-boot.sh stop' \
  [ $s = 0 -a ! -d "$R/sys/module/xnu" ]
check '... (the boot script was asked)' called '^xnulinux-boot.sh stop'

# the install paths are as they were: the fki installer still writes its unit and the command line
host idle; rm "$U/xnulinux-fki.service" "$G"
LINUX_MEM=3G CARVE_BASE=0x100000000 CARVE_MB=512 /bin/bash "$P/scripts/vm-install-fki-service.sh" > "$T/out" 2>&1; s=$?
check 'vm-install-fki-service.sh (no argument) installs as before' \
  [ $s = 0 -a "$(grep -c 'carve_base=0x100000000 carve_mb=512' "$U/xnulinux-fki.service")$(grep -c 'mem=3G' "$G")" = 11 ]
finish

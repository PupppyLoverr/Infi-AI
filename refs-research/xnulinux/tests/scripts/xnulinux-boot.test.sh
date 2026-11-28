#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/xnulinux-boot.sh (start, --wait, stop) and scripts/install-service.sh on a stand-in host. The scripts are the
# repository's, with one change made here: the system paths they name (/sys/module, /proc/irq, /run, /etc, /var/tmp,
# /var/log, /dev/dri, /sys/devices/system/cpu) start under a temporary directory. sudo runs its command; make, insmod,
# rmmod, dmesg, journalctl, systemctl are stand-ins. The stand-in insmod of xnu.ko "runs" as the real one does (it
# returns when the run ends) and ends as its scenario says: by a halt, a panic, a stop, or by not loading at all.
. "$(dirname "$0")/lib.sh"
[ "$(uname -s)" = Linux ] || { echo "skipped: xnulinux-boot.sh is for the Linux host (bash 4, GNU date, /proc)"; exit 0; }
! pgrep -f "insmod xnu-ko/xnu.ko" > /dev/null || { echo "skipped: a hosted system is being started on this machine"; exit 0; }
sandbox_repo
P=$T/repo; R=$T/root; export R
mkdir -p "$P/fki-ko" "$P/xnu-ko" "$P/gpu/xnu-drm"
echo 'echo "vm-net-up: stand-in"' > "$P/scripts/vm-net-up.sh"
for s in xnulinux-boot.sh install-service.sh; do	# the system paths, under $R
  sed -e "s|/sys/module|$R/sys/module|g" -e "s|/sys/devices/system/cpu|$R/sys/devices/system/cpu|g" -e "s|/proc/irq|$R/proc/irq|g" \
    -e "s|/proc/sys/vm|$R/proc/sys/vm|g" -e "s|/run/xnu-irq-affinity|$R/run/xnu-irq-affinity|g" -e "s|/etc/|$R/etc/|g" \
    -e "s|/var/tmp|$R/var/tmp|g" -e "s|/var/log/xnulinux|$R/var/log/xnulinux|g" -e "s|/dev/dri|$R/dev/dri|g" \
    "$REPO/scripts/$s" > "$P/scripts/$s"
done

stub sudo <<'EOF'
[ "${1:-}" != -k ] || { [ ! -e "$T/sudo-asks" ] || exit 1; shift; }	# -k: not on a password typed a moment ago
while [ $# -gt 0 ]; do case $1 in -n|-E) shift;; *=*) export "$1"; shift;; *) break;; esac; done
exec "$@"
EOF
stub make <<'EOF'
[ ! -e "$T/make-fails" ] || { echo "fki.c:1:1: error: the stand-in make was told to fail"; exit 2; }
for a in "$@"; do case $a in M=*) d=${a#M=}; n=$(basename "$d"); : > "$d/${n%-ko}.ko";; esac; done
EOF
stub modinfo <<'EOF'
echo SRC1
EOF
stub nproc <<'EOF'
echo 4
EOF
stub lscpu < /dev/null
stub sync < /dev/null
stub systemctl < /dev/null
stub systemd-run < /dev/null
stub sleep <<'EOF'
case $1 in 20) set -- 0.3;; 5) set -- 0.6;; 3|2|1) set -- 0.2;; esac	# the script's waits, shortened
PATH=${PATH#*:} exec sleep "$@"
EOF
stub dmesg <<'EOF'
case ${1:-} in -n) echo "$2" > "$T/conlevel";; -C) : > "$T/ring";; *) cat "$T/ring";; esac
EOF
stub journalctl <<'EOF'
case " $* " in *" -k "*) cat "$T/klog";; esac
EOF
cat > "$T/bin/klog" <<'EOF'	# one kernel log record, as the ring buffer and journald show it
#!/bin/bash
t=$(awk '{printf "%12.6f", $1}' /proc/uptime)
echo "[$t] $*" >> "$T/ring"; echo "[$t] host kernel: $*" >> "$T/klog"
EOF
chmod +x "$T/bin/klog"; export T
stub rmmod <<'EOF'
case ${RMMOD:-} in
lost) rm -rf "$R/sys/module/$1"; echo "rmmod: ERROR: Module $1 is not currently loaded" >&2; exit 1;;	# another rmmod got there first
busy) echo "rmmod: ERROR: Module $1 is in use" >&2; exit 1;;
esac
[ -d "$R/sys/module/$1" ] || { echo "rmmod: ERROR: Module $1 is not currently loaded" >&2; exit 1; }
rm -rf "$R/sys/module/$1"
EOF
stub insmod <<'EOF'
case $1 in
*fki.ko)
  mkdir -p "$R/sys/module/fki/parameters"; echo SRC1 > "$R/sys/module/fki/srcversion"; : > "$R/sys/module/fki/parameters/cpu_list"
  for a in "$@"; do case $a in carve_base=*|carve_mb=*) echo "${a#*=}" > "$R/sys/module/fki/parameters/${a%%=*}";; esac; done
  exit 0;;
esac
read -r how after < "$T/scenario"
case $how in
noload) echo "insmod: ERROR: could not insert module $1: Unknown symbol in module" >&2; exit 1;;	# before the module's init
refused) klog "fki: xnu: FKI caps 0x000000000000000c"; klog "fki: xnu: cannot read the trust cache, err 0xfffffffffffffffe"
  echo "insmod: ERROR: could not insert module $1: Unknown symbol in module" >&2; exit 1;;	# xnu.ko's init returned -2
esac
mkdir -p "$R/sys/module/xnu/parameters"; echo 0 > "$R/sys/module/xnu/parameters/stop_run"
klog "fki: xnu: mode=3: the kernel collection"; klog "fki: darwin console: xnulinux# "
s=$SECONDS
while :; do
  if [ "$(cat "$R/sys/module/xnu/parameters/stop_run" 2>/dev/null)" = 1 ]; then klog "fki: xnu: run stopped by the host (stop_run)"; exit 0; fi
  if [ -n "${after:-}" ] && [ $((SECONDS - s)) -ge "$after" ]; then
    case $how in
    halt) klog "fki: xnu: the hosted system halted: run ended";;
    panic) klog "fki: xnu: XNU panic:"; klog "fki: panic(cpu 0 caller 0xfffffe0000000000): stand-in";;
    longpanic) klog "fki: xnu: XNU panic:"	# and a long log after it
      seq 1 50000 | sed 's/^/[0.000000] host kernel: fki: xnu: monitor line /' >> "$T/klog";;
    esac
    exit 0	# the module stays loaded after a run that ended by itself
  fi
  sleep 0.1
done
EOF

host() {	# a host after its boot: fki.ko loaded by its unit (unless FKI=no), nothing running; $1: how xnu.ko's run will go
  pkill -f "$T/bin/insmod" 2>/dev/null; rm -rf "$R"
  mkdir -p "$R/sys/module" "$R/proc/irq/1" "$R/proc/irq/2" "$R/proc/sys/vm" "$R/run" "$R/etc/systemd/system" "$R/var/tmp" "$R/dev"
  for c in 0 1 2 3; do mkdir -p "$R/sys/devices/system/cpu/cpu$c/cpuidle/state1"; echo 0 > "$R/sys/devices/system/cpu/cpu$c/cpuidle/state1/disable"; done
  echo f > "$R/proc/irq/default_smp_affinity"; echo f > "$R/proc/irq/1/smp_affinity"; echo 3 > "$R/proc/irq/2/smp_affinity"
  printf 'ROOT_UUID=11111111-2222-3333-4444-555555555555\nBOOT_UUID=66666666-7777-8888-9999-000000000000\n' > "$R/var/tmp/mac.conf"
  : > "$T/ring"; : > "$T/klog"; : > "$T/calls"; rm -f "$T/conlevel"; echo "${1:-run}" > "$T/scenario"
  [ "${FKI:-}" = no ] || insmod fki-ko/fki.ko carve_base=0x200000000 carve_mb=8156
}
boot() { /bin/bash "$P/scripts/xnulinux-boot.sh" "$@" > "$T/out" 2>&1; }
is() { [ "$(cat "$1" 2>/dev/null)" = "$2" ]; }	# is FILE TEXT
log() { ls "$R"/var/log/xnulinux/run-*.log 2>/dev/null | head -1; }

# ---- a start, and a stop
host; boot; s=$?
check 'a start returns 0 once the hosted system runs, and says where its console is' [ $s = 0 -a "$(grep -c '^booting: ' "$T/out")" = 1 ]
check '... the console level lets an alert (an XNU panic) through to the VM console: 2' is "$T/conlevel" 2
check "... Linux's device interrupts are off XNU's CPU (CPUS=1 of 4: mask e)" is "$R/proc/irq/default_smp_affinity" e
check "... XNU's CPU is out of the deep idle states, the others are not" [ "$(cat "$R"/sys/devices/system/cpu/cpu[01]/cpuidle/state1/disable | tr -d '\n')" = 10 ]
boot stop; s=$?
check 'stop ends it: status 0, xnu.ko unloaded' [ $s = 0 -a ! -d "$R/sys/module/xnu" -a "$(grep -c '^hosted system stopped' "$T/out")" != 0 ]
check '... and gives the interrupts their CPUs back, each its own (f, f, 3)' \
  [ "$(cat "$R/proc/irq/default_smp_affinity" "$R/proc/irq/1/smp_affinity" "$R/proc/irq/2/smp_affinity" | tr '\n' ' ')" = "f f 3 " ]
check '... and the idle states' is "$R/sys/devices/system/cpu/cpu0/cpuidle/state1/disable" 0
check '... with nothing left in /run' [ ! -e "$R/run/xnu-irq-affinity" ]

# ---- a start whose xnu.ko does not load
host refused; boot; s=$?
check 'a start whose xnu.ko did not load fails' [ $s = 1 ]
check '... does not say "booting"' not grep -q '^booting' "$T/out"
check "... and says why in xnu.ko's own words, under insmod's" \
  [ "$(grep -A2 '^xnu.ko did not load: insmod: ERROR' "$T/out" | tail -1)" = "  fki: xnu: cannot read the trust cache, err 0xfffffffffffffffe" ]
host noload; boot; s=$?
check 'a start whose load Linux itself refused (nothing of xnu.ko in the kernel log): status 1, with insmod'"'"'s words' \
  [ $s = 1 -a "$(grep -c '^xnu.ko did not load: insmod: ERROR' "$T/out")" = 1 ]
check '... and the last thing it does is stop the GPU bridge (it did not end at the empty search of the kernel log)' \
  [ "$(tail -1 "$T/calls")" = "systemctl stop xnudrmd" ]
host noload; boot --wait; s=$?
check '--wait, the kernel having logged nothing of fki: status 3 and a message with the log' [ $s = 3 -a "$(grep -c "^the run ended: $R/var/log/xnulinux/run-" "$T/out")" != 0 ]
check '... and it says that xnu.ko did not load, as the plain start does, and not "booting"' \
  [ "$(grep -c '^xnu.ko did not load: insmod: ERROR' "$T/out")$(grep -c '^booting' "$T/out")" = 10 ]

# ---- --wait: how the run ended
host 'panic 1'; boot --wait; s=$?
check '--wait, an XNU panic: status 2, "panicked"' [ $s = 2 -a "$(grep -c '^the hosted system panicked: ' "$T/out")" != 0 ]
check "... the run's log is kept, with the panic" grep -q 'fki: xnu: XNU panic:' "$(log)"
check '... xnu.ko is unloaded and the interrupts are back (a run that ended by itself)' [ ! -d "$R/sys/module/xnu" -a "$(cat "$R/proc/irq/default_smp_affinity")" = f ]
host 'longpanic 1'; boot --wait; s=$?
check '--wait, an XNU panic with a long log after it: status 2, "panicked"' [ $s = 2 -a "$(grep -c '^the hosted system panicked: ' "$T/out")" != 0 ]
host 'halt 2'; /bin/bash "$P/scripts/xnulinux-boot.sh" --wait > "$T/out" 2>&1 & w=$!
wait_for 10 grep -q '^booting' "$T/out"
klog "fki: darwin console: # grep -c 'XNU panic' /var/log/notes"; klog "fki: darwin console: XNU panic: 3 lines"
wait $w; s=$?
check '--wait, a clean halt of a system whose console showed the words "XNU panic": status 0, "shut down"' \
  [ $s = 0 -a "$(grep -c '^the hosted system shut down: ' "$T/out")" != 0 ]

# ---- stop and the --wait process both unload xnu.ko (as under systemd: ExecStop, then SIGTERM to --wait)
host; boot
RMMOD=lost boot stop; s=$?
check "stop, xnu.ko unloaded by the other one between its test and its rmmod: not a failed stop" [ $s = 0 -a "$(grep -c '^hosted system stopped' "$T/out")" != 0 ]
host; boot
RMMOD=busy boot stop; s=$?
check "stop, xnu.ko really not unloading: status 1 with rmmod's message" [ $s = 1 -a "$(grep -c 'Module xnu is in use' "$T/out")" != 0 ]
host; /bin/bash "$P/scripts/xnulinux-boot.sh" --wait > "$T/out" 2>&1 & w=$!
wait_for 10 grep -q '^booting' "$T/out"
/bin/bash "$P/scripts/xnulinux-boot.sh" stop > "$T/out.stop" 2>&1; kill -TERM $w 2>/dev/null; wait $w; s=$?
check 'a stop of the service: --wait ends with status 0 and "stopped"' [ $s = 0 -a "$(grep -c '^the hosted system stopped: ' "$T/out")" != 0 ]

# ---- fki.ko not loaded at the start (its load at boot failed: after a kernel update it was the old kernel's)
FKI=no host; echo "ExecStart=/usr/sbin/insmod /opt/xnulinux/fki-ko/fki.ko carve_base=0x400000000 carve_mb=4060" > "$R/etc/systemd/system/xnulinux-fki.service"
boot; boot stop
check "fki.ko not loaded at the start: loaded with its boot unit's carve-out, not the default one" \
  called '^insmod fki-ko/fki.ko carve_base=0x400000000 carve_mb=4060'
FKI=no host; boot; boot stop
check '... and with the default one where there is no such unit (and the start goes on)' \
  [ "$(grep -c '^insmod fki-ko/fki.ko carve_base=0x200000000 carve_mb=8156' "$T/calls")$(grep -c '^insmod xnu-ko/xnu.ko ' "$T/calls")" = 11 ]

# ---- the installer
host; : > "$T/sudo-asks"
/bin/bash "$P/scripts/install-service.sh" > "$T/out" 2>&1; s=$?
check "install-service.sh, a user whose sudo asks for a password: refused, with the rule to add" [ $s = 1 -a "$(grep -c 'NOPASSWD' "$T/out")" != 0 ]
check '... and no unit written' [ ! -e "$R/etc/systemd/system/xnulinux.service" ]
rm "$T/sudo-asks"; /bin/bash "$P/scripts/install-service.sh" > "$T/out" 2>&1; s=$?
check 'install-service.sh with sudo that does not ask: the unit is written' [ $s = 0 -a -s "$R/etc/systemd/system/xnulinux.service" ]
check "... and it wants fki's boot unit, it does not require it (a failed load there must not keep the service down)" \
  [ "$(grep -c '^Wants=.*xnulinux-fki.service' "$R/etc/systemd/system/xnulinux.service")$(grep -c '^Requires=' "$R/etc/systemd/system/xnulinux.service")" = 10 ]
pkill -f "$T/bin/insmod" 2>/dev/null
finish

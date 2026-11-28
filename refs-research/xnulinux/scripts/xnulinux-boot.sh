#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM: boot the hosted macOS. Builds fki.ko and xnu.ko against the running kernel (stops on a compile error, never
# boots stale modules), brings up the hosted system's network (xnu0) and GPU bridge (xnudrmd, when there is a render node),
# and loads xnu.ko on the disk mk-macvm-disk.sh made. Returns once XNU is running; xnu.ko stays loaded until the hosted
# system halts or `xnulinux-boot.sh stop` ends it.
#
# Usage: xnulinux-boot.sh            boot it and return once XNU runs
#        xnulinux-boot.sh --wait     boot it and stay until the run ends (systemd: scripts/install-service.sh); the run's
#                                    kernel log goes to /var/log/xnulinux/, the exit status says how it ended
#        xnulinux-boot.sh stop       end it (the disk synced first)
#
# Configuration: /etc/xnulinux.conf, then $MAC_CONF (/var/tmp/mac.conf, written by mk-macvm-disk.sh: ROOT_UUID, BOOT_UUID), then the
# environment:
#   DISK (/var/tmp/mac.img)  RAM_MB (7900)  BOOTARGS  CONSOLE_IN (/var/tmp/console-in)  INPUT_IN (/var/tmp/xnu-input)
#   FB_DUMP (/var/tmp/xnu-fb.raw)  KEYSTORE (/var/tmp/xnu-keystore.bin)  TRUSTCACHE (xnu/trustcache-mac.bin; empty: none)
#   IMAGE (xnu/kc.bin: the kernel collection under /lib/firmware)  GPU (auto|off)  CPUS (1: the hosted system's CPUs, 32 at most)  XNU_ARGS (more xnu.ko parameters)
set -euo pipefail
# a relative MAC_CONF or DISK in the environment is a file where the caller is (docs/free-kc.md: MAC_CONF=pd.conf DISK=pd.img)
for v in MAC_CONF DISK; do [[ ${!v:-/} = /* ]] || printf -v "$v" %s "$PWD/${!v}"; done
cd "$(dirname "$0")/.."
[ -f /etc/xnulinux.conf ] && . /etc/xnulinux.conf	# for stop as well: the sync is typed into this system's CONSOLE_IN
irqs_back() {	# Linux's device interrupts back on the CPUs they had before the start took them off XNU's (/run: gone at a reboot, as they are)
  local f v; [ -s /run/xnu-irq-affinity ] || return 0
  while read -r f v; do [ -n "$v" ] && [ -e "$f" ] && echo "$v" | sudo tee "$f" >/dev/null 2>&1 || true; done < /run/xnu-irq-affinity
  sudo rm -f /run/xnu-irq-affinity
}
if [ "${1:-}" = stop ]; then	# end the running hosted system (its disk synced first) and remove xnu.ko
  if [ ! -d /sys/module/xnu ]; then
    sudo systemctl stop xnudrmd 2>/dev/null || true
    irqs_back
    echo "no hosted system is running"; exit 0
  fi
  printf 'sync\n' >> "${CONSOLE_IN:-/var/tmp/console-in}"; sleep 3
  echo 1 | sudo tee /sys/module/xnu/parameters/stop_run >/dev/null
  # every XNU CPU sees the stop at its next host call; one in a long one (disk I/O while the VM shuts down) takes a while
  for _ in $(seq 1 600); do pgrep -f "insmod xnu-ko/xnu.ko" >/dev/null || break; sleep 0.2; done
  # (under systemd --wait's collect unloads it too, and may get there between this test and the rmmod: not a failed stop)
  if [ -d /sys/module/xnu ]; then e=$(sudo rmmod xnu 2>&1) || [ ! -d /sys/module/xnu ] || { echo "$e" >&2; exit 1; }; fi
  sudo systemctl stop xnudrmd 2>/dev/null || true
  for f in /sys/devices/system/cpu/cpu*/cpuidle/state[1-9]*/disable; do [ -e "$f" ] && echo 0 | sudo tee "$f" >/dev/null 2>&1 || true; done	# XNU's CPUs are Linux's again
  irqs_back
  echo "hosted system stopped"
  exit
fi
WAIT=0; [ "${1:-}" = --wait ] && WAIT=1
MAC_CONF=${MAC_CONF:-/var/tmp/mac.conf}	# the disk's UUIDs: mk-macvm-disk.sh's, or mk-puredarwin-disk.sh's output
[ -f "$MAC_CONF" ] && . "$MAC_CONF"
DISK=${DISK:-/var/tmp/mac.img}; RAM_MB=${RAM_MB:-7900}; GPU=${GPU:-auto}; CPUS=${CPUS:-1}
CONSOLE_IN=${CONSOLE_IN:-/var/tmp/console-in}; INPUT_IN=${INPUT_IN:-/var/tmp/xnu-input}; FB_DUMP=${FB_DUMP:-/var/tmp/xnu-fb.raw}
KEYSTORE=${KEYSTORE:-/var/tmp/xnu-keystore.bin}; TRUSTCACHE=${TRUSTCACHE-xnu/trustcache-mac.bin}; IMAGE=${IMAGE:-xnu/kc.bin}
BOOTARGS=${BOOTARGS:-amfi_get_out_of_my_way=1 lp-sip0=2050 bt-transport-override=10}
: "${ROOT_UUID:?no ROOT_UUID (/var/tmp/mac.conf from mk-macvm-disk.sh)}" "${BOOT_UUID:?no BOOT_UUID}"
[ -d /sys/module/xnu ] && { echo "xnu.ko is loaded: a hosted system is running (sudo rmmod xnu first)" >&2; exit 1; }

K=/lib/modules/$(uname -r)/build
build() {	# a module; on failure its compiler errors, modpost's, and what make said when it stopped (no kernel headers)
  local log; log=$(make -s -C "$K" M="$PWD/$1" ${2:-} modules 2>&1) ||
    { { echo "$log" | grep -iE "error|\*\*\*" || echo "$log" | tail -5; } | head -20 >&2; exit 1; }
}
build fki-ko
build xnu-ko "KBUILD_EXTRA_SYMBOLS=$PWD/fki-ko/Module.symvers"
# XNU prints a lot (console lines, kprintf): to the ring buffer only (journalctl -k, scripts/vm-dmesg-shell.sh), not the
# VM console, where a CPU printing waits for the console with interrupts off. Level 2, not 1: the console takes what is
# below its level, and fki_log_alert's lines (an XNU panic, an XNU CPU gone silent) are KERN_ALERT (1)
sudo dmesg -n "${CONLEVEL:-2}"
# fki has the hosted kernel's DRAM from its load at boot (scripts/vm-install-fki-service.sh): reload it only when it changed,
# with the same carve-out
CARVE="carve_base=$(cat /sys/module/fki/parameters/carve_base 2>/dev/null || echo 0x200000000) carve_mb=$(cat /sys/module/fki/parameters/carve_mb 2>/dev/null || echo 8156)"
# Not loaded at all: its load at boot failed (after a kernel update fki.ko was still the old kernel's, until the build
# above). Then with what xnulinux-fki.service loads it with, which is where a host's own carve-out is written down.
if [ ! -d /sys/module/fki ]; then
  # (no such unit on a host where fki.ko is loaded by hand: sed's failure must not end the script here, under set -e)
  UNIT_ARGS=$(sed -n 's|^ExecStart=.*/fki-ko/fki\.ko  *||p' /etc/systemd/system/xnulinux-fki.service 2>/dev/null | head -1 || true)
  [ -z "$UNIT_ARGS" ] || CARVE=$UNIT_ARGS
fi
# (an fki.ko from before cpu_list has none: set -e must not stop here)
CPULIST=$(tr -d '\n' 2>/dev/null < /sys/module/fki/parameters/cpu_list || true); [ -z "$CPULIST" ] || CARVE="$CARVE cpu_list=$CPULIST"
if [ "$(cat /sys/module/fki/srcversion 2>/dev/null)" != "$(modinfo -F srcversion fki-ko/fki.ko)" ]; then
  sudo systemctl stop xnudrmd 2>/dev/null || true	# it holds fki's /dev/xnu-hostcall
  sudo rmmod fki 2>/dev/null || true
  sudo insmod fki-ko/fki.ko $CARVE ${FKI_ARGS:-}
fi
[ "$(cat /sys/module/fki/parameters/carve_mb 2>/dev/null)" != 0 ] ||
  echo "fki has no carve-out (scripts/vm-install-fki-service.sh): the allocation from CMA may fail" >&2
bash scripts/vm-net-up.sh

# the hosted system's files on the host: console and input queues, the framebuffer xnu.ko writes, the keystore
: > "$CONSOLE_IN"; : > "$INPUT_IN"; : > "$FB_DUMP"
rm -f "$KEYSTORE"; truncate -s 1M "$KEYSTORE"	# fresh each boot, as the disk was made (persisting it: production.md)
# XNU's DRAM is one physically contiguous block (GBs, from CMA): free the page cache and compact first. Right after the VM
# boots the allocation can still fail with the area free (a page in the way not yet migratable): wait and try again,
# for up to three minutes.
# Linux's device interrupts off the CPUs XNU takes (0..CPUS-1): a CPU running XNU's kernel holds Linux's interrupts
# until it next calls the host, so a disk completion routed there waits that long, and with several XNU CPUs one waiting
# for another's I/O can wait for good (as a hypervisor keeps device interrupts off its vCPUs' cores).
NCPU=$(nproc --all)
HOSTCPUS=	# Linux's CPUs that XNU does not take
if [ "$NCPU" -gt "$CPUS" ]; then
  # XNU's CPUs: Linux CPUs 0..CPUS-1, or the first CPUS of fki's cpu_list
  XCPUS=$(tr -d '\n' 2>/dev/null < /sys/module/fki/parameters/cpu_list || true)
  MASK=$(python3 -c "
l = [int(c) for c in '$XCPUS'.split(',') if c][:$CPUS] or list(range($CPUS))
m = (1 << $NCPU) - 1
for c in l: m &= ~(1 << c)
h = format(m, 'x')	# Linux reads a CPU mask in comma-separated groups of 32 bits
print(','.join(reversed([h[max(i - 8, 0):i] for i in range(len(h), 0, -8)])))")
  # what they were, for the stop or the end of the run to give back (kept when a start finds it there: the values are the first start's)
  [ -s /run/xnu-irq-affinity ] || for f in /proc/irq/default_smp_affinity /proc/irq/*/smp_affinity; do echo "$f $(cat "$f" 2>/dev/null)"; done | sudo tee /run/xnu-irq-affinity >/dev/null
  echo "$MASK" | sudo tee /proc/irq/default_smp_affinity >/dev/null 2>&1 || true
  for f in /proc/irq/*/smp_affinity; do echo "$MASK" | sudo tee "$f" >/dev/null 2>&1 || true; done
  HOSTCPUS=$(python3 -c "print(','.join(str(c) for c in range($NCPU) if 0x${MASK//,/} >> c & 1))")
fi
# XNU's CPUs stay out of Linux's deep idle states: a hosted CPU with nothing to run sleeps in Linux, and waking it from
# one (a thread made runnable on another CPU: every Mach message) took 0.6 ms on average on a DGX Spark, 10 us without.
XLIST=$(python3 -c "print(' '.join(str(c) for c in ([int(c) for c in '$(cat /sys/module/fki/parameters/cpu_list 2>/dev/null | tr -d '\n')'.split(',') if c][:$CPUS] or list(range($CPUS)))))")
for c in $XLIST; do
  for f in /sys/devices/system/cpu/cpu$c/cpuidle/state[1-9]*/disable; do [ -e "$f" ] && echo 1 | sudo tee "$f" >/dev/null 2>&1 || true; done
done
# Start xnudrmd BEFORE insmod xnu.ko and wait until it has opened /dev/xnu-hostcall and is serving:
# fast-booting guests (e.g. 16K pages) can open XnuDrm within ~4-5 s of insmod; restarting xnudrmd after
# insmod kills live DRM/Venus clients out from under WindowServer. Never restart xnudrmd while XNU runs.
sudo systemctl stop xnudrmd 2>/dev/null || true
sudo systemctl reset-failed xnudrmd 2>/dev/null || true
if [ "$GPU" != off ] && [ -e /dev/dri/renderD128 ]; then
  make -s -C gpu/xnu-drm >/dev/null
  # off XNU's CPUs, the render server it starts too: a thread woken on a CPU that XNU holds runs when XNU next calls the host
  sudo systemd-run -q --unit xnudrmd --collect ${HOSTCPUS:+-p CPUAffinity="$HOSTCPUS"} "$PWD/gpu/xnu-drm/xnudrmd"
  DRM_READY=0
  DRM_INV=$(systemctl show -p InvocationID --value xnudrmd 2>/dev/null || true)
  for _ in $(seq 1 100); do
    if [ -n "$DRM_INV" ] && sudo journalctl _SYSTEMD_INVOCATION_ID="$DRM_INV" -q --no-pager 2>/dev/null | grep "xnudrmd: serving" >/dev/null; then
      DRM_READY=1; break
    fi
    DRM_STATE=$(systemctl show -p ActiveState --value xnudrmd 2>/dev/null || true)
    [ "$DRM_STATE" = active ] || [ "$DRM_STATE" = activating ] || break
    sleep 0.1
  done
  if [ "$DRM_READY" = 1 ]; then
    echo "gpu: xnudrmd ready on /dev/dri/renderD128 (journalctl -u xnudrmd)"
    # The virglrenderer backend's shared memory is Linux's 4 KB pages, wherever they are: XnuDrm cannot map them into a
    # task whose VM pages are 16 KB (xnu.ko's default), so the hosted system gets 4 KB VM pages (override: XNU_ARGS)
    if sudo journalctl _SYSTEMD_INVOCATION_ID="$DRM_INV" -q --no-pager 2>/dev/null | grep "serving virglrenderer" >/dev/null; then
      VM16K=vm16k=0
    fi
    # GPU name for System Information (override: GPU_MODEL=)
    GPU_MODEL=${GPU_MODEL:-$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -1 || true)}
  else
    echo "gpu: xnudrmd did not become ready within 10 s (journalctl -u xnudrmd); continuing with GPU=off" >&2
    sudo systemctl stop xnudrmd 2>/dev/null || true
  fi
fi
# Chip name for System Information (override: SOC_NAME=)
XCPU0=$( (tr -d '\n' 2>/dev/null < /sys/module/fki/parameters/cpu_list || true) | cut -d, -f1); XCPU0=${XCPU0:-0}
SOC_NAME=${SOC_NAME:-$(lscpu -e=CPU,MODELNAME 2>/dev/null | awk -v c="$XCPU0" '$1 == c { $1 = ""; sub(/^ +/, ""); print }' || true)}
[ "$SOC_NAME" = - ] && SOC_NAME=	# lscpu's "-": the CPU has no model name (a guest)
NAMES=()
[ -n "${SOC_NAME:-}" ] && NAMES+=("soc_name=\"$SOC_NAME\"")
[ -n "${GPU_MODEL:-}" ] && NAMES+=("gpu_model=\"$GPU_MODEL\"")
# mode=3: the kernel collection; the run lasts until the hosted system halts.
# insmod returns when the run ends: it runs detached, its output in /var/tmp/xnu-insmod.log.
START=$(date +%s)	# the run's log (--wait) starts here, so a failure while xnu.ko loads is in it
for try in 1 2 3 4 5 6 7 8 9; do
  sync; echo 3 | sudo tee /proc/sys/vm/drop_caches >/dev/null; echo 1 | sudo tee /proc/sys/vm/compact_memory >/dev/null
  sudo dmesg -C
  sudo setsid nohup insmod xnu-ko/xnu.ko mode=3 cpus="$CPUS" ram_mb="$RAM_MB" image="$IMAGE" \
    ${TRUSTCACHE:+trustcache="$TRUSTCACHE"} disk="$DISK" root_uuid="$ROOT_UUID" boot_uuid="$BOOT_UUID" preboot_uuid="$BOOT_UUID" \
    console_in="$CONSOLE_IN" input_in="$INPUT_IN" fb_dump="$FB_DUMP" keystore="$KEYSTORE" bootargs="\"$BOOTARGS\"" "${NAMES[@]}" ${VM16K:-} ${XNU_ARGS:-} \
    > /var/tmp/xnu-insmod.log 2>&1 < /dev/null &
  sleep 5
  if sudo dmesg | grep -aE "fki: carve-out .* (overlaps|is not all RAM)" >/dev/null; then	# a configuration error: no retry
    sudo dmesg | grep -aE "fki: carve-out .* (overlaps|is not all RAM)" >&2
    sudo systemctl stop xnudrmd 2>/dev/null || true
    exit 1
  fi
  sudo dmesg | grep -a "contiguous allocation failed" >/dev/null || break
  if [ $try = 9 ]; then
    echo "no contiguous memory for XNU (CMA: $(grep CmaFree /proc/meminfo))" >&2
    sudo systemctl stop xnudrmd 2>/dev/null || true
    exit 1
  fi
  echo "no contiguous memory for XNU yet: trying again in 20 s"; sleep 20
done
# insmod gone and no xnu.ko: it did not load (no disk or collection, a parameter out of range)
if ! pgrep -f "insmod xnu-ko/xnu.ko" >/dev/null && [ ! -d /sys/module/xnu ]; then
  # with what xnu.ko itself said last: insmod only has the error number's words, and for a file xnu.ko did not find (the
  # collection, the trust cache, the disk: -2) those are "Unknown symbol in module"
  echo "xnu.ko did not load: $(cat /var/tmp/xnu-insmod.log)" >&2
  # (xnu.ko said nothing when Linux itself refused the load, as for a parameter longer than it takes: grep's status
  # must not end the script here, before the GPU bridge is stopped)
  { sudo dmesg | grep -a "fki: xnu: " | tail -3 | sed 's/^\[[^]]*\] */  /' >&2; } || true
  if [ $WAIT = 0 ]; then
    sudo systemctl stop xnudrmd 2>/dev/null || true
    exit 1
  fi
  # --wait has said why as well, and goes on to keep the run's log and to end with 3, as for any run that ended
else
  echo "booting: console scripts/xnu-console.sh, screen tools/hosted/fb2png.py $FB_DUMP"
  [ $WAIT = 1 ] || exit 0
fi

# --wait: until the run ends (insmod returns), then keep its kernel log and say how it ended: 0 a halt (from inside, or
# a stop asked for), 2 an XNU panic, 4 a restart the hosted system asked for, 3 the run ended otherwise
collect() {	# the run's kernel log, kept; the status says how it ended
  local LOGDIR=/var/log/xnulinux LOG
  # insmod has returned but xnu.ko stays loaded after a run that ended by itself (a panic, a halt): unload it, or the
  # next start (systemd's restart after a panic) finds it and refuses
  # (a stop that was asked for unloads it as well: whichever of the two gets there second finds it gone)
  if [ -d /sys/module/xnu ]; then sudo rmmod xnu 2>/dev/null || [ ! -d /sys/module/xnu ] || echo "xnu.ko did not unload" >&2; fi
  sudo systemctl stop xnudrmd 2>/dev/null || true
  # XNU's CPUs are Linux's again, as after a stop: a run that ended by itself otherwise left them out of the deep idle states
  for f in /sys/devices/system/cpu/cpu*/cpuidle/state[1-9]*/disable; do [ -e "$f" ] && echo 0 | sudo tee "$f" >/dev/null 2>&1 || true; done
  irqs_back
  sudo mkdir -p "$LOGDIR"
  LOG=$LOGDIR/run-$(date -d @"$START" +%Y%m%d-%H%M%S).log
  # (no line of fki's at all when xnu.ko did not load: set -e must not end the script here, before it says how the run ended)
  { cat /var/tmp/xnu-insmod.log; sudo journalctl -k -q -o short-monotonic --no-pager --since @"$START" | grep -a "fki:" || true; } |
    sudo tee "$LOG" >/dev/null
  ls -1t "$LOGDIR"/run-*.log | tail -n +21 | xargs -r sudo rm -f	# the last 20 runs
  # how it ended is in xnu.ko's own lines, not in what the hosted console printed (the log has those too: a console that
  # showed the words "XNU panic" turned a clean shutdown into a panic, and systemd started the system again)
  # (grep -c, not -q: under pipefail the first grep, cut off by an early -q, fails with SIGPIPE on a long log)
  said() { grep -av "fki: darwin console: " "$LOG" | grep -ac "$1" >/dev/null; }
  if said "XNU panic"; then echo "the hosted system panicked: $LOG" >&2; return 2; fi
  # shut down from inside (shutdown -h, the Apple menu): a halt is a clean end; a restart ends with 4 for systemd to start it again
  # (checked before the stop: the other CPUs log "run stopped by the host" as they leave after it)
  if said "xnu: the hosted system halted"; then echo "the hosted system shut down: $LOG"; return 0; fi
  if said "xnu: the hosted system asked for a restart"; then echo "the hosted system restarts: $LOG"; return 4; fi
  if [ "${1:-}" = stopped ] || said "run stopped by the host"; then echo "the hosted system stopped: $LOG"; return 0; fi
  echo "the run ended: $LOG" >&2; return 3
}
# systemctl stop: ExecStop (xnulinux-boot.sh stop) ends the run, then systemd sends this script SIGTERM
trap 'while pgrep -f "insmod xnu-ko/xnu.ko" >/dev/null; do sleep 1; done; collect stopped; exit $?' TERM
while pgrep -f "insmod xnu-ko/xnu.ko" >/dev/null; do sleep 2 & wait $!; done
collect; exit $?

#!/bin/bash
# Runs inside the Hyprland session: open every surface, screenshot it to the
# host, then measure idle RAM 5 minutes after login and report.
H=http://10.0.2.2:8088
T0=$(date +%s)
up() { curl -fsS -T "$1" "$H/$(basename "$1")" >/dev/null 2>&1; }
shot() { sleep "${2:-1.2}"; grim "/tmp/$1.png" && up "/tmp/$1.png"; echo "shot $1"; }
q() { qs ipc call shell "$@"; }
sleep 14
shot 01-desktop-idle-notch 0
mpv --no-video --idle=no --loop=inf --force-media-title="Harbour Lights" "av://lavfi:sine=frequency=220" >/dev/null 2>&1 &
sleep 3; shot 02-notch-live-activity 0
q island media; shot 03-island-media
for t in "https://github.com/PupppyLoverr/Infi-AI" "#0a84ff" "cargo build --release"; do printf '%s' "$t" | wl-copy; sleep 1.6; done
q island clip; shot 04-island-clipboard
q island shelf; shot 05-island-shelf
q island cal; shot 06a-island-calendar
q island face; shot 06b-island-notchface 2
q island tools; shot 06-island-tools
q island close; sleep 0.8
q hud volume 0.62; shot 07-hud-volume 0.5
sleep 2; touch "$HOME/Downloads/report.pdf"; q island shelf; shot 07b-tracked-downloads-on-shelf
q island close; sleep 1
sleep 2
# Two live activities at once: the timer rides in the island, the music
# splits off into its own bubble.
q timer 300; shot 07c-live-activities 2.0
q timer 0; sleep 0.8
q query "te"; shot 08-spotlight-in-island 1.6
q spotlight; sleep 0.5
q control; shot 09-control-center
q control; sleep 0.5
notify-send -a "Solyra" "Welcome to Solyra" "Notifications arrive in the corner and respect Focus."; shot 10-notification 1.0
sleep 5
q snap; shot 11-snap-layouts
q snap; sleep 0.5
solyra-term >/tmp/term1.log 2>&1 &
shot 12-terminal 6
qs ipc call shell overview; shot 12b-mission-control 1.5
qs ipc call shell overview; sleep 0.6
hyprctl dispatch 'hl.dsp.window.close()' >/dev/null; sleep 1
q glass 90; shot 14-glass-tinted 1.0
q glass 0; shot 15-glass-clear 1.0
solyra-files >/tmp/files.log 2>&1 &
shot 15b-finder 4
hyprctl dispatch 'hl.dsp.window.close()' >/dev/null; sleep 1
q glass 35
q island close; q lock; shot 16-lock-screen 2.5
# The test media player is part of the scenario, not the desktop: stop it,
# then idle until 5 minutes after login and measure (nothing excluded).
pkill -x mpv; qs ipc call shell island close
while [ $(( $(date +%s) - T0 + 14 )) -lt 300 ]; do sleep 5; done
{
  echo "=== solyra-vm report ==="
  echo "VM: 2 vCPU, 4 GiB RAM, virtio-vga (llvmpipe)"
  free -m
  systemd-analyze 2>&1 | head -2
  echo "root_used_mb=$(df -m --output=used / | tail -1)"
  echo "packages=$(pacman -Qq | wc -l)"
  echo "--- top RSS (KiB)"; ps -eo rss,comm --sort=-rss | head -20
  echo "total_rss_kib=$(ps -eo rss= | awk '{s+=$1} END{print s}')"
  hyprctl version | head -2
  qs --version 2>/dev/null | head -1
} > /tmp/report.txt 2>&1
up /tmp/report.txt
for f in /tmp/qs.log /tmp/hypr.log /tmp/scenario.log /tmp/term1.log /tmp/term2.log; do [ -f "$f" ] && cp "$f" "/tmp/log-$(basename $f)" && up "/tmp/log-$(basename $f)"; done
touch /tmp/solyra-done

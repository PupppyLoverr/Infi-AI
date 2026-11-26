#!/bin/bash
# Root-side watchdog for one CI boot: if the session has not produced its
# first screenshot after 150 s, send back why (Hyprland/Quickshell logs and
# the journal), then power off instead of idling to the timeout.
H=http://10.0.2.2:8088
up() { [ -f "$1" ] && curl -fsS -T "$1" "$H/$2" >/dev/null 2>&1; }
# Hyprland now reads hyprland.lua; try the legacy file explicitly first.
U=/home/solyra
if Hyprland --help 2>&1 | grep -q -- "-c"; then
  echo '[ "$(tty)" = /dev/tty1 ] && exec Hyprland -c ~/.config/hypr/hyprland.conf > /tmp/hypr.log 2>&1' > $U/.bash_profile
  chown solyra: $U/.bash_profile
fi
{
  echo "=== Hyprland --version"; Hyprland --version 2>&1 | head -5
  echo "=== Hyprland --help"; Hyprland --help 2>&1 | head -40
  echo "=== /usr/share/hypr"; find /usr/share/hypr /usr/share/hyprland* -maxdepth 3 2>/dev/null | head -50
  echo "=== config strings"; strings "$(command -v Hyprland)" 2>/dev/null | grep -iE "hyprland\.(conf|lua)|legacy|hyprlang" | sort -u | head -40
} > /tmp/hypr-info.txt 2>&1
up /tmp/hypr-info.txt hypr-info.txt
for f in $(find /usr/share/hypr /usr/share/hyprland* -name '*.lua' 2>/dev/null | head -5); do up "$f" "example-$(basename $f)"; done
sleep 150
if [ ! -f /tmp/01-desktop-idle-notch.png ]; then
  echo "=== solyra hook: no session after 150 s ==="
  ls -la /tmp
  tail -60 /tmp/hypr.log 2>/dev/null
  journalctl -b --no-pager -p warning | tail -60 > /tmp/journal.txt; cat /tmp/journal.txt
  ls -la /dev/dri 2>&1
  up /tmp/hypr.log log-hypr.log; up /tmp/qs.log log-qs.log; up /tmp/journal.txt journal.txt
  for f in /home/solyra/.local/share/hyprland/hyprland.log /run/user/*/hypr/*/hyprland.log; do up "$f" hyprland-full.log; done
  up $U/.config/hypr/hyprland.lua generated-hyprland.lua
  sync; poweroff
fi
# Hard stop: never run past 12 minutes.
sleep 570; [ -f /tmp/solyra-done ] || { up /tmp/qs.log log-qs.log; up /tmp/hypr.log log-hypr.log; up /tmp/scenario.log log-scenario.log; sync; poweroff; }

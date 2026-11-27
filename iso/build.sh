#!/bin/bash
# Build the Solyra ISO: archiso's releng profile + our packages + our
# airootfs overlay + this checkout's shell, terminal and configs.
# Runs as root in an Arch container: iso/build.sh <solyra-term binary> <out dir>
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd); term=$1; out=$2
work=$(mktemp -d); prof=$work/profile
pacman -Sy --noconfirm --needed archiso >/dev/null
cp -r /usr/share/archiso/configs/releng "$prof"
cat "$repo/iso/packages.solyra" | grep -v '^#' >> "$prof/packages.x86_64"
cp -r "$repo/iso/airootfs/." "$prof/airootfs/"
# greetd owns tty1: drop releng's root autologin there.
rm -f "$prof/airootfs/etc/systemd/system/getty@tty1.service.d/autologin.conf"
# Shell, Hyprland config, wallpapers, terminal.
install -Dm755 "$term" "$prof/airootfs/usr/local/bin/solyra-term"
mkdir -p "$prof/airootfs/etc/skel/.config/quickshell" "$prof/airootfs/etc/skel/.config/hypr" "$prof/airootfs/etc/skel/.config/solyra" "$prof/airootfs/usr/share/backgrounds/solyra"
cp -r "$repo/shell/qs/." "$prof/airootfs/etc/skel/.config/quickshell/"
cp "$repo/shell/hypr/hyprland.lua" "$prof/airootfs/etc/skel/.config/hypr/hyprland.lua"
cp "$repo"/assets/wallpapers/*-1920x1080.png "$repo"/assets/wallpapers/solyra-dusk-1920x1200.png "$prof/airootfs/usr/share/backgrounds/solyra/"
cp "$repo/assets/wallpapers/solyra-dusk-1920x1200.png" "$prof/airootfs/etc/skel/.config/solyra/wallpaper.png"
: > "$prof/airootfs/etc/skel/.config/solyra/hypr-local.lua"
install -Dm644 /dev/stdin "$prof/airootfs/usr/share/applications/solyra-term.desktop" <<'D'
[Desktop Entry]
Name=Terminal
Comment=Solyra Terminal
Exec=solyra-term
Icon=utilities-terminal
Type=Application
Categories=System;TerminalEmulator;
D
# Branding.
sed -i 's/^iso_name=.*/iso_name="solyra"/; s/^iso_label=.*/iso_label="SOLYRA_$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y%m)"/; s/^iso_publisher=.*/iso_publisher="Solyra"/; s/^iso_application=.*/iso_application="Solyra Live"/' "$prof/profiledef.sh"
grep -q 'solyra-install' "$prof/profiledef.sh" || sed -i 's|^file_permissions=(|file_permissions=(\n  ["/usr/local/bin/solyra-install"]="0:0:755"\n  ["/usr/local/bin/solyra-live-session"]="0:0:755"\n  ["/usr/local/bin/solyra-term"]="0:0:755"|' "$prof/profiledef.sh"
mkarchiso -v -w "$work/w" -o "$out" "$prof" 2>&1 | tail -15
ls -la "$out"

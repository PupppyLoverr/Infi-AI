#!/usr/bin/env bash
# build.sh — produce a bootable raw x86_64 CosmosOS disk image.
#
# Pipeline:
#   1. cargo build --release --workspace (skip with SKIP_CARGO_BUILD=1)
#   2. mmdebstrap a Debian trixie minimal rootfs + kernel + runtime deps
#   3. lay down the Cosmos overlay (binaries, .desktop files, session plumbing)
#   4. assemble a partitioned raw image (GPT: FAT32 ESP + btrfs with
#      @/@home/@snapshots/@var_log subvolumes, GRUB2-EFI under OVMF)
#
# Output: cosmosos/dist/cosmosos-x86_64.raw   (sparse, ~3G)
# Boot it with cosmosos/image/run.sh (OVMF pflash — UEFI only now).
#
# Requires the host deps from provision/host-deps.sh plus libpixman-1-dev,
# socat (test tooling), btrfs-progs/dosfstools, grub-efi-amd64-bin (the
# host's grub-install --target=x86_64-efi writes the ESP bootloader) and
# OVMF (run.sh's firmware).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COSMOS="$ROOT/cosmos"
WORK="$ROOT/work/image"
DIST="$ROOT/dist"
OVERLAY="$WORK/overlay"
ROOTFS="$WORK/rootfs"
IMG="${IMG:-$DIST/cosmosos-x86_64.raw}"
IMG_SIZE="${IMG_SIZE:-3G}"
SUITE="${SUITE:-trixie}"
MIRROR="${MIRROR:-http://deb.debian.org/debian}"

# --- 1. build the workspace ---------------------------------------------------

# cosmos-portal's libspa-sys bindgen needs pipewire headers newer than
# Ubuntu 22.04's (libspa-0.9 wants spa_video_info_raw.flags). provision/
# host-deps.sh fetches trixie's dev+runtime debs into ~/pipewire-prefix and
# writes a hybrid .pc: trixie headers for bindgen + host libdir for the link
# (trixie's .so requires glibc 2.38; host glibc is 2.35; the guest's trixie
# .so satisfies the binary at runtime).
export PKG_CONFIG_PATH="${PIPEWIRE_PKG_CONFIG:-$HOME/pipewire-prefix/hybrid}:${PKG_CONFIG_PATH:-}"

# pam-sys (cosmos-lock) drives bindgen via clang-sys, which dlopens
# libclang at runtime — Ubuntu splits it under /usr/lib/llvm-*/lib and
# clang-sys's search misses it without a hint.
export LIBCLANG_PATH="${LIBCLANG_PATH:-$(dirname "$(find /usr/lib -name 'libclang.so*' -xtype f 2>/dev/null | sort -V | head -1)")}"
if [ "${SKIP_CARGO_BUILD:-0}" != "1" ]; then
  echo "== cargo build --release --workspace =="
  cargo build --release --workspace --manifest-path "$COSMOS/Cargo.toml"
fi
BINDIR="$COSMOS/target/release"
for b in cosmos-compositor cosmos-shell cosmos-files cosmos-terminal \
         cosmos-editor cosmos-settings cosmos-monitor cosmos-portal \
         cosmos-lock cosmos-greeter cosmos-agentd cosmos-agents cosmos-kit-gallery; do
  [ -x "$BINDIR/$b" ] || { echo "missing binary: $BINDIR/$b" >&2; exit 1; }
done

# --- 2. stage the overlay tree ------------------------------------------------

echo "== staging overlay =="
sudo rm -rf "$OVERLAY"   # previous run's files are chowned root below
mkdir -p "$OVERLAY"/{usr/local/bin,usr/share/applications,etc/profile.d,etc/skel,etc/systemd/network,etc/systemd/system/getty@tty1.service.d,etc/polkit-1/rules.d,etc/sudoers.d}

install -m755 "$BINDIR"/cosmos-{compositor,shell,files,terminal,editor,settings,monitor,portal,lock,greeter,agentd,agents,kit-gallery} \
  "$OVERLAY/usr/local/bin/"
install -m644 "$COSMOS"/apps/*/cosmos-*.desktop "$OVERLAY/usr/share/applications/"

# xdg-desktop-portal backend: cosmos.portal picks our impl when
# XDG_CURRENT_DESKTOP=cosmos; the .service lets dbus activation start
# cosmos-portal on demand (SystemdService names the user unit we ship).
mkdir -p "$OVERLAY/usr/share/xdg-desktop-portal/portals" \
         "$OVERLAY/usr/share/dbus-1/services" \
         "$OVERLAY/usr/lib/systemd/user"
install -m644 "$COSMOS/portal/data/cosmos.portal" \
  "$OVERLAY/usr/share/xdg-desktop-portal/portals/"
install -m644 "$COSMOS/portal/data/org.freedesktop.impl.portal.desktop.cosmos.service" \
  "$OVERLAY/usr/share/dbus-1/services/"
cat > "$OVERLAY/usr/lib/systemd/user/cosmos-portal.service" <<'EOF'
[Unit]
Description=cosmos xdg-desktop-portal backend
After=dbus.service

[Service]
Type=dbus
BusName=org.freedesktop.impl.portal.desktop.cosmos
ExecStart=/usr/local/bin/cosmos-portal

[Install]
WantedBy=default.target
EOF

# kiosk privilege path: the cosmos account's password is locked, so sudo
# password prompts could never be answered. NOPASSWD for the single user is
# the Omarchy-style answer for a live/kiosk image — lets `sudo apt/dpkg`
# work for installing downloaded apps. Documented in docs/security.md.
cat > "$OVERLAY/etc/sudoers.d/cosmos" <<'EOF'
cosmos ALL=(ALL:ALL) NOPASSWD: ALL
EOF
chmod 0440 "$OVERLAY/etc/sudoers.d/cosmos"

# firefox on Wayland: auto-detects our wl compositor when this is set.
cat > "$OVERLAY/etc/profile.d/50-firefox-wayland.sh" <<'EOF'
export MOZ_ENABLE_WAYLAND=1
EOF

# opencode — standalone AI coding agent (github.com/sst/opencode), shipped
# as the upstream single linux-x64 binary, no node/npm. "latest" resolves
# through the releases/latest redirect (no API, no token); pin a reproducible
# build with OPENCODE_VERSION=<tag>. The tarball is cached under work/ so
# rebuilds don't re-download ~60MB.
OC_VER="${OPENCODE_VERSION:-latest}"
if [ "$OC_VER" = latest ]; then
  OC_VER="$(curl -fsSIL -o /dev/null -w '%{url_effective}' \
    https://github.com/sst/opencode/releases/latest | sed 's|.*/||')"
  [ -n "$OC_VER" ] || { echo "could not resolve latest opencode tag" >&2; exit 1; }
fi
echo "opencode: $OC_VER"
mkdir -p "$WORK/cache"
OC_TGZ="$WORK/cache/opencode-linux-x64-$OC_VER.tar.gz"
[ -f "$OC_TGZ" ] || curl -fsSL -o "$OC_TGZ" \
  "https://github.com/sst/opencode/releases/download/$OC_VER/opencode-linux-x64.tar.gz"
OC_TMP="$(mktemp -d)"
tar -xzf "$OC_TGZ" -C "$OC_TMP" opencode
install -m755 "$OC_TMP/opencode" "$OVERLAY/usr/local/bin/opencode"
# host-side sanity: the staged binary must at least report its version
"$OVERLAY/usr/local/bin/opencode" --version >/dev/null || \
  { echo "opencode --version failed on staged binary" >&2; exit 1; }
rm -rf "$OC_TMP"

# xwayland-satellite — no Debian package (trixie) and not on crates.io;
# build from upstream git on the host and ship the binary. It owns :0 and
# lazily spawns the real Xwayland server when the first X11 client connects.
# Host needs: libxcb-cursor-dev libxcb-keysyms1-dev libxcb-icccm4-dev
# libxcb-ewmh-dev libxcb-render-util0-dev libxcb-util-dev (pkg-config).
XWS_BIN="$WORK/xwayland-sat/bin/xwayland-satellite"
if [ ! -x "$XWS_BIN" ]; then
  cargo install xwayland-satellite --locked \
    --git https://github.com/Supreeeme/xwayland-satellite \
    --root "$WORK/xwayland-sat"
fi
install -m755 "$XWS_BIN" "$OVERLAY/usr/local/bin/xwayland-satellite"

# Ambient wallpapers (generated by cosmos-wallgen; committed PNGs).
install -d "$OVERLAY/usr/share/cosmos/wallpapers"
if [ -d "$COSMOS/wallpapers" ]; then
  install -m644 "$COSMOS/wallpapers/"*.png "$OVERLAY/usr/share/cosmos/wallpapers/" 2>/dev/null || true
fi

# spec §6.11 skills — agent-facing OS-layout docs, shared system-wide.
install -d "$OVERLAY/usr/share/cosmos/skills"
if [ -d "$COSMOS/skills" ]; then
  install -m644 "$COSMOS/skills/"*.md "$OVERLAY/usr/share/cosmos/skills/" 2>/dev/null || true
fi

# opencode -> cosmos-agentd MCP wiring (Phase-6 demo out of the box):
# skel so every new user gets it. opencode's `mcp` object lists local
# servers as {type:local, command:[...]}.
install -d "$OVERLAY/etc/skel/.config/opencode"
cat > "$OVERLAY/etc/skel/.config/opencode/opencode.json" <<'EOF'
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "cosmos": {
      "type": "local",
      "command": ["cosmos-agentd", "--stdio"],
      "environment": {"COSMOS_AGENT": "Cosmos Helper"},
      "enabled": true
    }
  }
}
EOF

# Cosmos Helper — the shipped demo agent: opencode through cosmos-agentd
# (COSMOS_AGENT above), file tools only, scoped to ~/Documents; writes and
# moves ask first. agentd expands ~ per user.
install -d "$OVERLAY/etc/skel/.config/cosmos/agents" "$OVERLAY/etc/skel/Documents"
cat > "$OVERLAY/etc/skel/.config/cosmos/agents/Cosmos Helper.toml" <<'EOF'
command = "opencode"
read_roots = ["~/Documents"]
write_roots = ["~/Documents"]
tools = ["files.read", "files.search", "files.write", "files.move"]
sensitive = ["files.write", "files.move"]
EOF

# Seed content so a new account looks lived in (image/skel/): real
# Documents (welcome guide, its PDF rendering, a budget CSV), four
# generated photos in Pictures, and a small Rust project. Both
# generators are stdlib-only Python on the build host.
SEED="$ROOT/image/skel"
install -d "$OVERLAY/etc/skel/Pictures" "$OVERLAY/etc/skel/Projects"
install -m644 "$SEED/Documents/"* "$OVERLAY/etc/skel/Documents/"
python3 "$SEED/mkpdf.py" "$SEED/Documents/Welcome to CosmosOS.md" \
  "$OVERLAY/etc/skel/Documents/CosmosOS User Guide.pdf"
python3 "$SEED/mkphotos.py" "$OVERLAY/etc/skel/Pictures"
cp -r "$SEED/hello-cosmos" "$OVERLAY/etc/skel/Projects/"

# launcher/dock entry. Exec wraps opencode in cosmos-terminal -e (>= 910d3ab:
# args join into a command line run via $SHELL -c) so the TUI gets a real TTY.
cat > "$OVERLAY/usr/share/applications/opencode.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=opencode
Comment=AI coding agent
Exec=cosmos-terminal -e opencode
Icon=cosmos-terminal
Terminal=false
Categories=Utility;
EOF

# session wrapper: agetty -> login shell on tty1 -> profile.d -> this script.
# compositor exits -> we power off (appliance-style boot-to-desktop).
cat > "$OVERLAY/usr/local/bin/cosmos-session" <<'EOF'
#!/bin/bash
# CosmosOS session wrapper. Runs as user `cosmos` on tty1 via
# /etc/profile.d/99-cosmos-session.sh after agetty --autologin.
# Output goes through logger -> journald; the kernel cmdline sets
# systemd.journald.forward_to_console=1 so it also lands on /dev/console
# (ttyS0 -> run.sh's serial log). Writing to /dev/console directly as an
# unprivileged user fails (0600 root) and would kill this shell.
exec > >(logger -t cosmos-session) 2>&1

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
# cosmos.portal's UseIn=cosmos matches this: xdg-desktop-portal routes
# Screenshot/PickColor to our backend only on our desktop. The frontend
# runs under systemd --user, so push it into the manager + dbus
# activation environment, not just our process env.
export XDG_CURRENT_DESKTOP="cosmos"
dbus-update-activation-environment --systemd XDG_CURRENT_DESKTOP 2>/dev/null || \
  echo "cosmos-session: dbus-update-activation-environment failed (portal may not match UseIn)"
export WAYLAND_DISPLAY=""

echo "cosmos-session: starting cosmos-compositor on tty $(tty)"
cosmos-compositor --tty-udev &
COMP_PID=$!

# wait for the wayland socket (smithay picks first free wayland-N)
SOCKET=""
for _ in $(seq 1 100); do
  for s in "$XDG_RUNTIME_DIR"/wayland-*; do
    [ -S "$s" ] && { SOCKET="$s"; break; }
  done
  [ -n "$SOCKET" ] && break
  kill -0 "$COMP_PID" 2>/dev/null || break   # compositor died early
  sleep 0.2
done

if [ -n "$SOCKET" ]; then
  export WAYLAND_DISPLAY="${SOCKET##*/}"
  # XWayland via xwayland-satellite: satellite owns :0 and spawns the real
  # Xwayland server ONLY when the first X11 client connects (lazy). Export
  # DISPLAY so terminal-spawned X11 apps find it.
  export DISPLAY=":0"
  if command -v xwayland-satellite >/dev/null 2>&1; then
    xwayland-satellite :0 &
    echo "cosmos-session: xwayland-satellite :0 started (XWayland stays off until first X11 client)"
  fi
  echo "cosmos-session: wayland socket $WAYLAND_DISPLAY up, starting cosmos-shell"
  cosmos-shell &
  # opt-in smoke rig: present only when /etc/cosmos-smoke-apps flag exists
  # (created post-build by loop-mounting the image; not shipped enabled)
  if [ -f /etc/cosmos-smoke-apps ] && [ -x /usr/local/bin/cosmos-smoke-apps ]; then
    echo "cosmos-session: smoke flag present, starting cosmos-smoke-apps"
    /usr/local/bin/cosmos-smoke-apps &
  fi
else
  echo "cosmos-session: no wayland socket appeared (compositor died?)"
fi

wait "$COMP_PID"
rc=$?
# compositor exit -> log out. agetty's respawn then re-runs autologin and
# restarts the session: a built-in crash-retry loop. (poweroff was tried
# first; logind denies it for the unprivileged user without polkit.)
echo "cosmos-session: compositor exited ($rc) — logging out (session restarts on respawn)"
sleep 2
exit "$rc"
EOF
chmod 755 "$OVERLAY/usr/local/bin/cosmos-session"

# opt-in smoke rig: launches two app clients on the live compositor, logs
# guest `free -m`, probes the IPC socket (ping/list_windows/list_workspaces
# — newline-delimited JSON, see cosmos/ipc), then kills the apps and lists
# windows again to prove cleanup. Installed always; RUNS only when the
# /etc/cosmos-smoke-apps flag file exists — inject it by loop-mounting the
# finished image (`touch mnt/etc/cosmos-smoke-apps`).
cat > "$OVERLAY/usr/local/bin/cosmos-smoke-apps" <<'EOF'
#!/bin/bash
exec > >(logger -t cosmos-smoke) 2>&1
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
IPC="$XDG_RUNTIME_DIR/cosmos-ipc.sock"
probe() { echo "$1" | socat -t 3 - UNIX-CONNECT:"$IPC" || echo "ipc probe failed: $1"; }

echo "cosmos-smoke: warming up for shell"
sleep 6

echo "cosmos-smoke: launching all five apps"
PIDS=""
for app in cosmos-terminal cosmos-files cosmos-editor cosmos-settings cosmos-monitor; do
  "$app" & PIDS="$PIDS $!"
  sleep 2
done
sleep 8

echo "cosmos-smoke: guest memory (free -m):"
free -m

echo "cosmos-smoke: ipc ping:"
probe '{"op":"ping"}'
echo "cosmos-smoke: ipc list_windows (apps up):"
probe '{"op":"list_windows"}'
echo "cosmos-smoke: ipc list_workspaces:"
probe '{"op":"list_workspaces"}'

sleep 4
echo "cosmos-smoke: killing apps ($PIDS)"
kill $PIDS 2>/dev/null
sleep 3
echo "cosmos-smoke: ipc list_windows (post-kill):"
probe '{"op":"list_windows"}'
echo "cosmos-smoke: done"
EOF
chmod 755 "$OVERLAY/usr/local/bin/cosmos-smoke-apps"

# greetd takes tty1 (its unit conflicts getty@tty1); no agetty autologin
# drop-in and no profile.d session hook are shipped anymore — the greeter
# launches cosmos-session via greetd's StartSession. cosmos-session itself
# stays: it is the session Exec.

# greeter/session-lock plumbing — greetd on tty1 replaced agetty autologin:
# cage runs cosmos-greeter fullscreen; greetd StartSession launches
# cosmos-session for the signed-in user.
mkdir -p "$OVERLAY/etc/greetd" "$OVERLAY/usr/share/wayland-sessions" "$OVERLAY/etc/pam.d"
# greetd owns tty1 exclusively — Debian still enables getty@tty1 which
# restarts on the same VT and was seen racing the greeter (serial: getty@tty1
# restart counter at the same second the greeter respawned over a live
# session). Mask it.
sudo ln -sf /dev/null "$OVERLAY/etc/systemd/system/getty@tty1.service"
cat > "$OVERLAY/etc/greetd/config.toml" <<'EOF'
[terminal]
vt = 1

[default_session]
command = "cage -s -- cosmos-greeter"
user = "cosmos"
EOF
cat > "$OVERLAY/usr/share/wayland-sessions/cosmos.desktop" <<'EOF'
[Desktop Entry]
Name=Cosmos
Comment=CosmosOS Wayland session
Exec=cosmos-session
Type=Application
DesktopNames=cosmos
EOF
# greetd's session phase runs pam_systemd explicitly so logind registers
# the wayland session (XDG_SESSION_TYPE/CLASS come from the greeter's
# StartSession env) — loginctl list-sessions/lock-session depend on it.
cat > "$OVERLAY/etc/pam.d/greetd" <<'EOF'
#%PAM-1.0
auth     include  common-auth
account  include  common-account
password include  common-password
session  required pam_loginuid.so
session  required pam_limits.so
session  required pam_env.so readenv=1
session  required pam_env.so readenv=1 envfile=/etc/default/locale
session  required pam_unix.so
session  optional pam_systemd.so
EOF
# cosmos-lock authenticates via pam::Client("cosmos-lock") — without this
# service file every unlock attempt fails before pam_unix runs.
cat > "$OVERLAY/etc/pam.d/cosmos-lock" <<'EOF'
auth include common-auth
account include common-account
EOF

# cosmos-agentd — MCP agent runtime, a per-user service for the signed-in
# session. /etc/systemd/user covers every user; the unit is enabled for
# cosmos via a default.target.wants link.
mkdir -p "$OVERLAY/etc/systemd/user" "$OVERLAY/etc/systemd/user/default.target.wants"
cat > "$OVERLAY/etc/systemd/user/cosmos-agentd.service" <<'EOF'
[Unit]
Description=CosmosOS agent runtime
After=graphical-session.target

[Service]
ExecStart=/usr/local/bin/cosmos-agentd
Restart=on-failure

[Install]
WantedBy=default.target
EOF
ln -sf /etc/systemd/user/cosmos-agentd.service \
  "$OVERLAY/etc/systemd/user/default.target.wants/cosmos-agentd.service"

# NetworkManager owns every en*/eth* link (auto-DHCP "Wired connection") —
# the shell tray reads NM over D-Bus, so NM must hold the real interface.
# systemd-networkd stays enabled but has NO .network files: it manages
# nothing and serves as the documented fallback if NM is ever removed.
# No 20-uplink.network on purpose.

echo "cosmosos" > "$OVERLAY/etc/hostname"

# papercuts: locale default, zram swap, motd-free login
# /etc/default/locale is written by update-locale inside setup.sh (writing
# it here is too early — the locales postinst regenerates it). The
# profile.d drop covers non-PAM entry points (the compositor session reads
# profile.d too). LC_ALL is intentionally absent — it overrides everything
# and would block a future per-user locale setting.
printf 'export LANG=en_US.UTF-8\n' > "$OVERLAY/etc/profile.d/40-locale.sh"

# zram swap: lz4, capped at half of RAM. systemd-zram-generator reads this
# at boot and creates /dev/zram0 as a swap device.
mkdir -p "$OVERLAY/etc/systemd"
cat > "$OVERLAY/etc/systemd/zram-generator.conf" <<'EOF'
[zram0]
zram-size = ram / 2
compression-algorithm = lz4
EOF

# motd-free clean login: empty motd (pam_motd prints it verbatim) and a
# skel .hushlogin so the cosmos user skips all login banners.
: > "$OVERLAY/etc/motd"
touch "$OVERLAY/etc/skel/.hushlogin"

# snapper root config — SUSE-style layout: the @snapshots subvolume is
# mounted at /.snapshots by fstab; snapper adopts it for the "root"
# config (create-config can't run at build time: the rootfs isn't a
# btrfs mount inside the chroot). Timeline snapshots via
# snapper-timeline.timer (enabled in setup.sh); Debian's snapper also
# ships an apt pre/post hook at /etc/apt/apt.conf.d/80snapper.
mkdir -p "$OVERLAY/etc/snapper/configs"
cat > "$OVERLAY/etc/snapper/configs/root" <<'EOF'
FSTYPE="btrfs"
SUBVOLUME="/"
QGROUP=""
SPACE_LIMIT="0.5"
FREE_LIMIT="0.2"
ALLOW_USERS="cosmos"
ALLOW_GROUPS=""
SYNC_ACL="yes"
BACKGROUND_COMPARISON="yes"
NUMBER_CLEANUP="yes"
NUMBER_MIN_AGE="1800"
NUMBER_LIMIT="10"
NUMBER_LIMIT_IMPORTANT="5"
TIMELINE_CREATE="yes"
TIMELINE_CLEANUP="yes"
TIMELINE_MIN_AGE="1800"
TIMELINE_LIMIT_HOURLY="5"
TIMELINE_LIMIT_DAILY="7"
TIMELINE_LIMIT_WEEKLY="0"
TIMELINE_LIMIT_MONTHLY="0"
TIMELINE_LIMIT_YEARLY="0"
EMPTY_PRE_POST_CLEANUP="yes"
EMPTY_PRE_POST_MIN_AGE="1800"
EOF

# First-boot NVRAM registration: the image boots via the removable
# fallback (EFI/BOOT/BOOTX64.EFI) before any boot entry exists. This
# oneshot registers 'CosmosOS' in efivars once — OVMF vars.fd keeps
# NVRAM across boots of the same run.sh session, so efibootmgr -v
# shows the entry from then on. Stamped, no-op after first run.
mkdir -p "$OVERLAY/usr/local/sbin" "$OVERLAY/etc/systemd/system/multi-user.target.wants"
cat > "$OVERLAY/usr/local/sbin/cosmos-efi-bootentry.sh" <<'EOF'
#!/bin/sh
# Register the CosmosOS EFI boot entry in NVRAM (once per vars.fd).
exec >/dev/console 2>&1 || true
STAMP=/var/lib/cosmos/efi-bootentry.done
[ -f "$STAMP" ] && exit 0
[ -d /sys/firmware/efi/efivars ] || exit 0   # not an EFI boot
mkdir -p /var/lib/cosmos
if efibootmgr | grep -q CosmosOS; then
  touch "$STAMP"; exit 0
fi
efibootmgr --create --disk /dev/vda --part 1 \
  --label CosmosOS --loader '\\EFI\\cosmosos\\grubx64.efi' && touch "$STAMP"
EOF
chmod 755 "$OVERLAY/usr/local/sbin/cosmos-efi-bootentry.sh"
mkdir -p "$OVERLAY/usr/local/sbin" "$OVERLAY/etc/systemd/system/multi-user.target.wants"
cat > "$OVERLAY/etc/systemd/system/cosmos-efi-bootentry.service" <<'EOF'
[Unit]
Description=Register CosmosOS EFI boot entry in NVRAM (first boot)
ConditionPathExists=/sys/firmware/efi/efivars
After=local-fs.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/cosmos-efi-bootentry.sh

[Install]
WantedBy=multi-user.target
EOF
ln -sf ../cosmos-efi-bootentry.service \
  "$OVERLAY/etc/systemd/system/multi-user.target.wants/cosmos-efi-bootentry.service"

# --- curated shell env (Omarchy-style): lands in /home/cosmos/.bashrc via
# useradd -m's skel copy (hook runs after overlay sync-in).
cat > "$OVERLAY/etc/skel/.bashrc" <<'EOF'
# ~/.bashrc — CosmosOS curated terminal env

# interactive only — keeps non-interactive shells (agents, opencode tools)
# fast and clean
case $- in
  *i*) ;;
  *) return ;;
esac

export EDITOR=nvim
export VISUAL=nvim
export HISTCONTROL=ignoreboth
shopt -s checkwinsize

# colored prompt: blue user@hostname, cyan cwd
PS1='\[\e[1;34m\]\u@\h\[\e[0m\]:\[\e[1;36m\]\w\[\e[0m\]\$ '

# Debian ships fd as fdfind and bat as batcat — restore upstream names
command -v fdfind >/dev/null && alias fd='fdfind'
command -v batcat >/dev/null && alias bat='batcat'

command -v eza >/dev/null && {
  alias ll='eza -la --group-directories-first'
  alias la='eza -a'
  alias ls='eza'
}
command -v batcat >/dev/null && alias cat='batcat --paging=never'
command -v rg >/dev/null && alias grep='rg'

alias vim='nvim'

# Omarchy-style welcome banner on shell spawn (~60ms on this box — cheap
# enough to keep; drop the line if it ever feels laggy)
command -v fastfetch >/dev/null && fastfetch
EOF

# netdev group may call every org.freedesktop.NetworkManager.* action: the
# autologin session isn't always 'active' to polkit, and the stock Debian rule
# only grants settings.modify.system to netdev/sudo. The quick-settings NM
# toggle (nm networking off) needs network-control.
cat > "$OVERLAY/etc/polkit-1/rules.d/60-cosmos-nm.rules" <<'EOF'
polkit.addRule(function(action, subject) {
    if (action.id.indexOf("org.freedesktop.NetworkManager.") === 0 &&
        subject.isInGroup("netdev")) {
        return polkit.Result.YES;
    }
});
EOF

# cosmos-agents Rollback button runs `pkexec snapper -c root rollback <n>`:
# grant the cosmos user pkexec-exec on snapper only.
cat > "$OVERLAY/etc/polkit-1/rules.d/61-cosmos-snapper.rules" <<'EOF'
polkit.addRule(function(action, subject) {
    if (action.id === "org.freedesktop.policykit.exec" &&
        subject.user === "cosmos" &&
        action.lookup("program") === "/usr/bin/snapper") {
        return polkit.Result.YES;
    }
});
EOF

# snapper home config: create-config needs the live btrfs /home, so it
# runs once at first boot. Agents → Rollback restores files from these
# snapshots; cosmos-agentd takes one before an agent's first change in a
# session. ALLOW_USERS lets the cosmos user list/create without pkexec.
install -d "$OVERLAY/usr/local/libexec"
cat > "$OVERLAY/usr/local/libexec/cosmos-snapper-setup" <<'EOF'
#!/bin/sh
set -e
snapper -c home create-config /home
snapper -c home set-config ALLOW_USERS=cosmos SYNC_ACL=yes TIMELINE_CREATE=no \
  NUMBER_LIMIT=20 NUMBER_LIMIT_IMPORTANT=10
snapper -c home create --cleanup-algorithm number --description "First boot"
snapper -c root create --cleanup-algorithm number --description "First boot"
EOF
chmod 755 "$OVERLAY/usr/local/libexec/cosmos-snapper-setup"
cat > "$OVERLAY/etc/systemd/system/cosmos-snapper-setup.service" <<'EOF'
[Unit]
Description=Create the snapper home config on first boot
ConditionPathExists=!/etc/snapper/configs/home
After=local-fs.target dbus.service
Before=greetd.service

[Service]
Type=oneshot
ExecStart=/usr/local/libexec/cosmos-snapper-setup

[Install]
WantedBy=multi-user.target
EOF

# systemd-resolved's LLMNR binds 0.0.0.0:5355 (tcp+udp) by default — a real
# external listener and a spoofing surface on shared networks. The kiosk
# resolves DNS via DHCP, so LLMNR/mDNS stay off.
mkdir -p "$OVERLAY/etc/systemd/resolved.conf.d"
cat > "$OVERLAY/etc/systemd/resolved.conf.d/no-llmnr.conf" <<'EOF'
[Resolve]
LLMNR=no
MulticastDNS=no
EOF

# sysctl hardening (see docs/security.md). Verified live defaults already cover
# dmesg_restrict, protected_hardlinks/symlinks, syncookies, unprivileged bpf —
# pinned here so they can't drift with a Debian default change.
mkdir -p "$OVERLAY/etc/sysctl.d"
cat > "$OVERLAY/etc/sysctl.d/90-cosmos.conf" <<'EOF'
# kernel pointer/info leaks
kernel.kptr_restrict = 2
kernel.dmesg_restrict = 1
# protected_* in world-writable sticky dirs (/tmp, /dev/shm)
fs.protected_fifos = 2
fs.protected_regular = 2
fs.protected_hardlinks = 1
fs.protected_symlinks = 1
# ipv4 spoof sanity on the single uplink
net.ipv4.conf.all.rp_filter = 1
net.ipv4.conf.default.rp_filter = 1
net.ipv4.tcp_syncookies = 1
# no unprivileged bpf (2 = hard disable, can't be reset at runtime)
kernel.unprivileged_bpf_disabled = 2
EOF

# overlay files are system files — they must land in the rootfs owned by
# root, not mapped to the build host's uid (which is uid 1000 = cosmos
# in-guest).
sudo chown -R root:root "$OVERLAY"

# --- 3. mmdebstrap the rootfs -------------------------------------------------
# split-package reality check (trixie): networkd lives in `systemd`,
# resolved is `systemd-resolved`. No X11, no desktop environment.
# Mesa DRI gives llvmpipe (swrast) so virtio-gpu EGL works without vulkan.

PACKAGES="systemd-sysv udev dbus libpam-systemd kmod \
linux-image-amd64 initramfs-tools systemd-resolved \
network-manager polkitd pipewire pipewire-alsa wireplumber upower bluez \
libudev1 libxkbcommon0 libwayland-server0 libwayland-client0 \
libwayland-egl1 libwayland-cursor0 libdrm2 libgbm1 libegl1 libgles2 \
libgl1-mesa-dri libinput10 libseat1 libdisplay-info2 libpixman-1-0 \
libgudev-1.0-0 libdbus-1-3 \
fonts-dejavu-core fontconfig xdg-utils kbd procps mesa-utils socat login \
iproute2 \
ripgrep fd-find fzf eza bat btop fastfetch neovim tmux lazygit htop jq tree \
firefox-esr adwaita-icon-theme fonts-liberation fonts-inter fonts-jetbrains-mono \
sudo wget ca-certificates dbus-x11 xdg-user-dirs libfuse2t64 \
xdg-desktop-portal dbus-user-session libpipewire-0.3-0t64 libglib2.0-bin \
locales systemd-zram-generator \
grub-efi-amd64 btrfs-progs snapper efibootmgr \
xwayland x11-apps \
wl-clipboard pcmanfm \
greetd cage \
libxcb-cursor0 libxcb-image0 libxcb-render-util0 libxcb-util1 libxcb1 libxcb-render0 libxcb-shm0"

# in-chroot setup. NOTE: mmdebstrap hooks run on the HOST with $1=rootfs —
# guest commands must go through `chroot "$1"` (a bare useradd here creates
# the user on the build host!). Kept as a file for debuggability.
cat > "$WORK/setup.sh" <<'EOF'
#!/bin/sh
set -e
for g in video input render tty netdev; do
  getent group "$g" >/dev/null 2>&1 || groupadd -r "$g"
done
useradd -m -s /bin/bash -G video,input,render,tty,netdev cosmos
# greeter + lock authenticate against the cosmos account — a live-distro
# default password is required or PAM can never succeed (was locked).
echo 'cosmos:cosmos' | chpasswd
systemctl enable greetd.service || true
systemctl enable systemd-networkd.service systemd-resolved.service || true
ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf

# locale: generate + default en_US.UTF-8 (LC_ALL deliberately unset so a
# per-user override can win). LOCALE is the knob a future locale picker
# rewrites. update-locale writes /etc/default/locale AFTER package
# configuration — writing the file from the overlay is too early (the
# locales postinst regenerates it empty).
LOCALE=en_US.UTF-8
sed -i "s/^# *${LOCALE} UTF-8/${LOCALE} UTF-8/" /etc/locale.gen
locale-gen "$LOCALE"
update-locale LANG="$LOCALE"

# snapper: register the root config — Debian discovers configs via
# SNAPPER_CONFIGS in /etc/default/snapper, not the configs/ dir alone.
sed -i 's/^SNAPPER_CONFIGS=.*/SNAPPER_CONFIGS="root"/' /etc/default/snapper
systemctl enable snapper-timeline.timer snapper-cleanup.timer || true
systemctl enable cosmos-snapper-setup.service || true

# btrfs in the initramfs: the initrd is generated while the rootfs still
# lives on the host's ext4 — fstype autodetection would emit ext4-only
# tools. MODULES=most covers btrfs anyway; pin it for determinism.
echo btrfs >> /etc/initramfs-tools/modules
update-initramfs -u -k all

rm -f /tmp/setup.sh
EOF

echo "== mmdebstrap $SUITE =="
sudo rm -rf "$ROOTFS"
sudo mmdebstrap --variant=required --architectures=amd64 \
  --include="$PACKAGES" \
  --customize-hook="sync-in '$OVERLAY' /" \
  --customize-hook="copy-in '$WORK/setup.sh' /tmp" \
  --customize-hook='chroot "$1" sh /tmp/setup.sh' \
  "$SUITE" "$ROOTFS" "$MIRROR"

# --- 4. assemble the raw image ------------------------------------------------

echo "== assembling $IMG =="
mkdir -p "$DIST"
MNT="$WORK/mnt"
mkdir -p "$MNT"

truncate -s "$IMG_SIZE" "$IMG"
# GPT: p1 = EFI system partition (FAT32), p2 = btrfs root (subvolumes below).
parted -s "$IMG" mklabel gpt
parted -s "$IMG" mkpart ESP fat32 1MiB 257MiB
parted -s "$IMG" set 1 esp on
parted -s "$IMG" mkpart root btrfs 257MiB 100%

LOOP=""
TOP="$WORK/mnt-top"
cleanup() {
  [ -n "$LOOP" ] && sudo umount -R "$MNT" 2>/dev/null || true
  [ -n "$LOOP" ] && sudo umount "$TOP" 2>/dev/null || true
  [ -n "$LOOP" ] && sudo losetup -d "$LOOP" 2>/dev/null || true
}
trap cleanup EXIT

LOOP="$(sudo losetup -fP --show "$IMG")"
echo "loop: $LOOP"
sudo mkfs.vfat -F32 -n COSMOSEFI "${LOOP}p1"
sudo mkfs.btrfs -f -L cosmosos "${LOOP}p2"
ROOT_UUID="$(sudo blkid -s UUID -o value "${LOOP}p2")"
ESP_UUID="$(sudo blkid -s UUID -o value "${LOOP}p1")"
echo "btrfs UUID: $ROOT_UUID  esp UUID: $ESP_UUID"

# subvolumes at the btrfs top level: @ = rootfs, @home, @snapshots (mounted
# at /.snapshots — the snapper location), @var_log at /var/log.
mkdir -p "$TOP"
sudo mount "${LOOP}p2" "$TOP"
sudo btrfs subvolume create "$TOP/@"
sudo btrfs subvolume create "$TOP/@home"
sudo btrfs subvolume create "$TOP/@snapshots"
sudo btrfs subvolume create "$TOP/@var_log"
sudo umount "$TOP"

# mount the layout, then rsync the rootfs in — the mountpoints route each
# tree into its own subvolume (/home -> @home, /var/log -> @var_log, ...).
sudo mount -o subvol=@ "${LOOP}p2" "$MNT"
sudo mkdir -p "$MNT/home" "$MNT/.snapshots" "$MNT/var/log" "$MNT/boot/efi"
sudo mount -o subvol=@home "${LOOP}p2" "$MNT/home"
sudo mount -o subvol=@snapshots "${LOOP}p2" "$MNT/.snapshots"
sudo mount -o subvol=@var_log "${LOOP}p2" "$MNT/var/log"
sudo mount "${LOOP}p1" "$MNT/boot/efi"

sudo rsync -aHAX "$ROOTFS/" "$MNT/"

# fstab: btrfs subvol mounts + the ESP. Pass 0 on btrfs (no fsck at boot).
sudo tee "$MNT/etc/fstab" <<EOF
UUID=$ROOT_UUID  /            btrfs  rw,noatime,subvol=@           0 1
UUID=$ROOT_UUID  /home        btrfs  rw,noatime,subvol=@home       0 2
UUID=$ROOT_UUID  /.snapshots  btrfs  rw,noatime,subvol=@snapshots  0 2
UUID=$ROOT_UUID  /var/log     btrfs  rw,noatime,subvol=@var_log    0 2
UUID=$ESP_UUID   /boot/efi    vfat   rw,umask=0077                 0 1
EOF

KERNEL="$(basename "$(ls "$MNT"/boot/vmlinuz-* | sort -V | tail -1)")"
INITRD="$(basename "$(ls "$MNT"/boot/initrd.img-* | sort -V | tail -1)")"
echo "kernel: $KERNEL  initrd: $INITRD"

# GRUB2-EFI via grub-mkimage, NOT grub-install: the host distro's
# grub-install hardcodes distributor 'ubuntu' into the embedded prefix
# (core.img looks for /EFI/ubuntu/grub.cfg) — verified by booting to a
# grub> prompt. Building core.img directly puts a real bootstrap config
# inside the binary: find the btrfs root by UUID and chain the real
# grub.cfg at /@/boot/grub/grub.cfg.
GRUB_EMBED_CFG="$WORK/grub-embed.cfg"
cat > "$GRUB_EMBED_CFG" <<EOF
search --no-floppy --fs-uuid --set=root $ROOT_UUID
set prefix=(\$root)/@/boot/grub
export prefix
configfile \$prefix/grub.cfg
EOF
sudo mkdir -p "$MNT/boot/efi/EFI/BOOT" "$MNT/boot/efi/EFI/cosmosos" "$MNT/boot/grub"
# -d: build against the GUEST's own module set (Debian grub-efi-amd64-bin),
# not the host's — Ubuntu's modules map linux->linuxefi (shim loader) which
# breaks under plain OVMF.
sudo grub-mkimage -O x86_64-efi \
  -d "$MNT/usr/lib/grub/x86_64-efi" \
  -o "$MNT/boot/efi/EFI/BOOT/BOOTX64.EFI" \
  -p /EFI/BOOT -c "$GRUB_EMBED_CFG" \
  part_gpt btrfs fat normal configfile search search_fs_uuid linux \
  gzio efi_gop all_video boot chain echo eval test ls halt cat
# same binary answers the NVRAM entry \EFI\cosmosos\grubx64.efi
sudo cp "$MNT/boot/efi/EFI/BOOT/BOOTX64.EFI" \
        "$MNT/boot/efi/EFI/cosmosos/grubx64.efi"

# NOTE: no video= kernel arg on purpose — virtio-gpu takes its mode list
# from QEMU's advertised EDID, and run.sh picks the preferred mode via
# -device virtio-vga,xres=,yres= (GUEST_RES). A video= pin here would be
# baked at build time and could not follow per-boot resolution requests.
# Paths are /@/-prefixed: the btrfs default subvolume stays top-level, so
# GRUB traverses the @ subvol dir for the kernel; rootflags=subvol=@ is
# what mounts @ as / in the initramfs.
sudo tee "$MNT/boot/grub/grub.cfg" <<EOF
set default="0"
set timeout=2
insmod part_gpt
insmod btrfs

menuentry "CosmosOS" {
    search --no-floppy --fs-uuid --set=root $ROOT_UUID
    linux /@/boot/$KERNEL root=UUID=$ROOT_UUID rootfstype=btrfs rootflags=subvol=@ rw console=tty0 console=ttyS0,115200 systemd.journald.forward_to_console=1
    initrd /@/boot/$INITRD
}
EOF

sudo umount -R "$MNT"
sudo losetup -d "$LOOP"
LOOP=""
trap - EXIT

echo "== done: $IMG ($(du -h "$IMG" | cut -f1) real, ${IMG_SIZE} sparse) =="

#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On the Mac host: keep the Linux VM running under krunkit as a launchd agent of this user. krunkit exits when the guest
# reboots or powers off; launchd starts it again (KeepAlive), and at login (RunAtLoad). With the hosted macOS as a service
# in the VM (scripts/install-service.sh) a reboot of either comes back by itself.
#   scripts/install-krun-agent.sh            install and start (again, for new settings: the VM is stopped as remove stops it)
#   scripts/install-krun-agent.sh remove     stop the VM (the guest shut down first) and remove the agent
# Settings: the environment of scripts/krun-vm.sh (CPUS, MEM, DISK, SSH_PORT), written into the agent at install.
set -euo pipefail
P=$(cd "$(dirname "$0")/.." && pwd)
L=org.xnulinux.krun-vm
domain() { launchctl print "gui/$(id -u)" >/dev/null 2>&1 && echo "gui/$(id -u)" || echo "user/$(id -u)"; }	# no login session: the user domain
PL=$HOME/Library/LaunchAgents/$L.plist
S=${S:-$HOME/.local/state/xnupoc-krun}
# the VM as the agent runs it: the SSH_PORT and KRUN written into the agent at install
setting() { local v; v=$([ -f "$PL" ] && /usr/libexec/PlistBuddy -c "Print :EnvironmentVariables:$1" "$PL" 2>/dev/null) && echo "$v" || true; }
stop_vm() {	# stop_vm SSH_PORT KRUN: a clean guest shutdown first (the hosted macOS's disk synced by xnulinux.service), then the agent
  local k p key=${VMKEY:-$P/../vmkey}; [ -f "$key" ] || key=	# the key scripts/vm.sh uses, unless VMKEY names another
  # The wait is for one of the krunkits that run now to end: the VM that is told to power off. The moment it has, launchd
  # starts the VM again (KeepAlive): a wait for no krunkit at all met that one, sat out its two minutes, and the agent's
  # removal then killed a guest that had just booted.
  k=$(pgrep -f "${2:-$HOME/.local/krunkit}/bin/krunkit" || true)
  ssh -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -p "${1:-2223}" \
    ${key:+-i "$key"} admin@127.0.0.1 'sudo systemctl poweroff' 2>/dev/null || true
  for _ in $(seq 1 600); do [ -n "$k" ] || break; for p in $k; do kill -0 "$p" 2>/dev/null || break 2; done; sleep 0.2; done
  launchctl bootout "$(domain)/$L" 2>/dev/null || true
}
if [ "${1:-}" = remove ]; then
  stop_vm "${SSH_PORT:-$(setting SSH_PORT)}" "${KRUN:-$(setting KRUN)}"	# unless the environment says
  rm -f "$PL"; echo "removed"; exit 0
fi
# installed already (new settings): the VM the agent runs is stopped the same way first, with the settings it runs with
if launchctl print "$(domain)/$L" >/dev/null 2>&1; then stop_vm "$(setting SSH_PORT)" "$(setting KRUN)"; fi
mkdir -p "$(dirname "$PL")" "$S"
env_xml() { for v in CPUS MEM DISK SSH_PORT KRUN; do [ -n "${!v:-}" ] && printf '    <key>%s</key><string>%s</string>\n' "$v" "${!v}"; done; true; }
cat > "$PL" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$L</string>
  <key>ProgramArguments</key><array><string>/bin/bash</string><string>$P/scripts/krun-vm.sh</string></array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin</string>
$(env_xml)
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <!-- launchd runs an agent as a background process by default: its vCPU threads get the low QoS, and on a busy Mac the
       guest stalls (timers, its network) -->
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>$S/agent.log</string>
  <key>StandardErrorPath</key><string>$S/agent.log</string>
</dict>
</plist>
PLIST
launchctl bootout "$(domain)/$L" 2>/dev/null || true
launchctl bootstrap "$(domain)" "$PL"
echo "installed: $L (log $S/agent.log); scripts/install-krun-agent.sh remove to stop it"

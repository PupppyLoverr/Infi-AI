#!/bin/bash
# What the trimmed measurement removes (printed so the report can list it).
echo "=== TRIM ==="
rm=$(pacman -Qq discover packagekit packagekit-qt6 kdeconnect plasma-browser-integration drkonqi \
  plasma-welcome kwalletmanager print-manager plasma-firewall plasma-thunderbolt plasma-disks \
  plasma-vault kinfocenter 2>/dev/null)
echo "removing packages: $rm"
[ -n "$rm" ] && pacman -Rns --noconfirm $rm >/dev/null
echo "file indexing (baloo): off"
mkdir -p /home/solyra/.config
printf '[Basic Settings]\nIndexing-Enabled=false\n' > /home/solyra/.config/baloofilerc
chown -R solyra: /home/solyra/.config
echo "disabled services: $(for s in bluetooth cups avahi-daemon ModemManager power-profiles-daemon; do systemctl is-enabled $s >/dev/null 2>&1 && systemctl disable $s >/dev/null 2>&1 && printf '%s ' $s; done)"
pacman -Scc --noconfirm >/dev/null
echo "=== TRIM END ==="

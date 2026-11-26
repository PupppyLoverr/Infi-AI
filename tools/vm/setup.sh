#!/bin/bash
# First boot only (cached by CI): Arch + Hyprland + Quickshell + runtime deps.
set -x
pacman -Sy --noconfirm --needed archlinux-keyring
pacman -Su --noconfirm
pacman -S --noconfirm --needed hyprland quickshell qt6-declarative qt6-svg qt6-5compat \
  mesa vulkan-swrast swaybg grim wl-clipboard wtype brightnessctl networkmanager bluez-utils \
  pipewire wireplumber pipewire-pulse playerctl mpv mpv-mpris libnotify upower \
  inter-font ttf-jetbrains-mono noto-fonts curl xdg-utils dbus foot
useradd -m -G wheel,video,input -s /bin/bash solyra
echo 'solyra:solyra' | chpasswd
mkdir -p /etc/systemd/system/getty@tty1.service.d
printf '[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin solyra --noclear %%I $TERM\n' > /etc/systemd/system/getty@tty1.service.d/autologin.conf
echo '[ "$(tty)" = /dev/tty1 ] && exec Hyprland > /tmp/hypr.log 2>&1' > /home/solyra/.bash_profile
chown solyra: /home/solyra/.bash_profile
install -m755 /root/sync.sh /usr/local/bin/solyra-sync
printf '[Unit]\nAfter=network-online.target\nWants=network-online.target\nBefore=getty@tty1.service\n[Service]\nType=oneshot\nExecStart=/usr/local/bin/solyra-sync\n[Install]\nWantedBy=multi-user.target\n' > /etc/systemd/system/solyra-sync.service
systemctl enable solyra-sync.service NetworkManager upower
touch /etc/cloud/cloud-init.disabled
pacman -Scc --noconfirm
echo SETUP-DONE > /dev/ttyS0
poweroff

#!/bin/bash
# First boot (cloud-init): minimal Plasma 6 Wayland with tty autologin.
set -x
pacman -Sy --noconfirm --needed archlinux-keyring
pacman -Su --noconfirm
pacman -S --noconfirm --needed plasma-desktop mesa konsole
useradd -m -G wheel,video,input -s /bin/bash solyra
mkdir -p /etc/systemd/system/getty@tty1.service.d
printf '[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin solyra --noclear %%I $TERM\n' > /etc/systemd/system/getty@tty1.service.d/autologin.conf
echo '[ "$(tty)" = /dev/tty1 ] && exec /usr/lib/plasma-dbus-run-session-if-needed /usr/bin/startplasma-wayland' > /home/solyra/.bash_profile
chown solyra: /home/solyra/.bash_profile
install -m755 /root/measure.sh /usr/local/bin/measure.sh
printf '[Unit]\nAfter=multi-user.target\n[Service]\nType=simple\nExecStart=/usr/local/bin/measure.sh\n[Install]\nWantedBy=multi-user.target\n' > /etc/systemd/system/measure.service
systemctl enable measure.service
mkdir -p /var/lib/measure; echo stock > /var/lib/measure/stage
touch /etc/cloud/cloud-init.disabled
pacman -Scc --noconfirm
echo SETUP-DONE > /dev/ttyS0
poweroff

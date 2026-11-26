#!/bin/bash
# First boot (cloud-init): Arch + Hyprland + Quickshell (bar + dock prototype), tty autologin.
set -x
pacman -Sy --noconfirm --needed archlinux-keyring
pacman -Su --noconfirm
pacman -S --noconfirm --needed hyprland quickshell qt6-declarative mesa foot pipewire wireplumber ttf-dejavu
useradd -m -G wheel,video,input -s /bin/bash solyra
mkdir -p /etc/systemd/system/getty@tty1.service.d /home/solyra/.config/hypr /home/solyra/.config/quickshell
printf '[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin solyra --noclear %%I $TERM\n' > /etc/systemd/system/getty@tty1.service.d/autologin.conf
echo '[ "$(tty)" = /dev/tty1 ] && exec Hyprland > /tmp/hypr.log 2>&1' > /home/solyra/.bash_profile
cat > /home/solyra/.config/hypr/hyprland.conf <<'C'
exec-once = quickshell
decoration { rounding = 16
  blur { enabled = true; size = 8; passes = 2 } }
animations { enabled = true }
misc { disable_hyprland_logo = true }
C
cat > /home/solyra/.config/quickshell/shell.qml <<'Q'
import Quickshell
import QtQuick
ShellRoot {
  SystemClock { id: clock; precision: SystemClock.Minutes }
  PanelWindow {
    anchors { top: true; left: true; right: true }
    implicitHeight: 30; color: "#33000000"
    Text { anchors.centerIn: parent; color: "white"; text: Qt.formatDateTime(clock.date, "ddd MMM d  hh:mm") }
  }
  PanelWindow {
    anchors.bottom: true; implicitWidth: 640; implicitHeight: 76; color: "transparent"; exclusiveZone: 0
    Rectangle { anchors.fill: parent; anchors.margins: 6; radius: 22; color: "#40ffffff"; border.color: "#55ffffff"
      Row { anchors.centerIn: parent; spacing: 10
        Repeater { model: 9; Rectangle { width: 48; height: 48; radius: 12; color: Qt.hsla(index / 9, 0.6, 0.55, 1) } } } }
  }
}
Q
chown -R solyra: /home/solyra
install -m755 /root/measure.sh /usr/local/bin/measure.sh
printf '[Unit]\nAfter=multi-user.target\n[Service]\nType=simple\nExecStart=/usr/local/bin/measure.sh\n[Install]\nWantedBy=multi-user.target\n' > /etc/systemd/system/measure.service
systemctl enable measure.service
mkdir -p /var/lib/measure; echo stock > /var/lib/measure/stage; echo quickshell > /var/lib/measure/proc
touch /etc/cloud/cloud-init.disabled
pacman -Scc --noconfirm
echo SETUP-DONE > /dev/ttyS0
poweroff

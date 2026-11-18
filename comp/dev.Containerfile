# Dev container for the Rust stack (comp/, shell/, apps/, agent/).
# Fedora 46 to match the guest glibc the binaries run against.
# CI uses this to typecheck niri before it becomes cosmos-comp.
FROM quay.io/fedora/fedora:46

RUN dnf -y install \
      rust cargo \
      clang-devel pkgconf-pkg-config \
      wayland-devel libxkbcommon-devel libinput-devel systemd-devel \
      libdisplay-info-devel libseat-devel \
      mesa-libgbm-devel mesa-libEGL-devel libglvnd-devel \
      pipewire-devel pulseaudio-libs-devel \
      cairo-gobject-devel pango-devel \
 && dnf -y clean all

WORKDIR /src

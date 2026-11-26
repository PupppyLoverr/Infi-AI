#!/bin/bash
# CI driver: (cached) base image, payload from this checkout, boot, collect.
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd); here=$repo/tools/vm
out=${OUT:-$PWD/out}; mkdir -p "$out/payload" "$out/shots"; cd "$out"
if [ ! -f base.qcow2 ]; then
  curl -fsSL -o base.qcow2 https://geo.mirror.pkgbuild.com/images/latest/Arch-Linux-x86_64-cloudimg.qcow2
  qemu-img resize -q base.qcow2 16G
  {
    echo '#cloud-config'; echo 'write_files:'
    for f in setup sync; do
      echo "  - path: /root/$f.sh"; echo "    permissions: '0755'"; echo "    encoding: b64"
      echo "    content: $(base64 -w0 "$here/$f.sh")"
    done
    echo 'runcmd:'; echo '  - [bash, /root/setup.sh]'
  } > user-data
  printf 'instance-id: solyra\nlocal-hostname: solyra\n' > meta-data
  cloud-localds seed.img user-data meta-data
  timeout 45m qemu-system-x86_64 -accel kvm -cpu host -m 4096 -smp 2 -display none -device virtio-vga \
    -drive file=base.qcow2,if=virtio -drive file=seed.img,if=virtio,format=raw \
    -nic user,model=virtio -serial file:setup.log -monitor none || true
  tail -2 setup.log
fi
# Payload: binaries, desktop entries, the QML shell, Hyprland config, wallpaper, scenario.
P=payload/stage; rm -rf $P; mkdir -p $P/bin $P/share/applications
cp "${TERM_BIN:-$repo/apps/target/release/solyra-term}" $P/bin/solyra-term
printf '[Desktop Entry]\nName=Terminal\nComment=Solyra Terminal\nExec=solyra-term\nIcon=utilities-terminal\nType=Application\n' > $P/share/applications/solyra-term.desktop
cp -r "$repo/shell/qs" $P/qs; cp -r "$repo/shell/hypr" $P/hypr
cp "$repo/assets/wallpapers/forest-1920x1080.png" $P/wallpaper.png
cp "$here/scenario.sh" "$here/hook.sh" $P/
tar -C $P -cf payload/payload.tar .
python3 "$here/serve.py" payload shots & SRV=$!
cp base.qcow2 run.qcow2
# Prefer a virgl GPU (Hyprland's happy path); the runner has no GPU, so try a
# vgem render node with Mesa's software EGL, and fall back to plain virtio.
sudo modprobe vgem 2>/dev/null && sudo chmod 666 /dev/dri/* 2>/dev/null || true
boot() {
  timeout 16m qemu-system-x86_64 -accel kvm -cpu host -m 4096 -smp 2 $1 \
    -drive file=run.qcow2,if=virtio -nic user,model=virtio -serial file:boot.log -monitor none
}
start=$(date +%s)
echo "GPU: virgl (egl-headless)"
boot "-device virtio-vga-gl -display egl-headless" || true
if [ $(( $(date +%s) - start )) -lt 30 ]; then
  echo "GPU: virgl unavailable here, using plain virtio-vga"
  cp base.qcow2 run.qcow2
  boot "-device virtio-vga -display none" || echo "VM ended ($?)"
fi
kill $SRV || true
rm -f run.qcow2
ls shots; cat shots/report.txt 2>/dev/null || { grep -a -A80 "solyra hook" boot.log | head -120; tail -5 boot.log; }

#!/bin/bash
# Milestone 1 measurement: minimal Arch + Plasma 6 Wayland in QEMU/KVM,
# 2 vCPU / 4 GiB RAM, virtio-gpu (llvmpipe rendering), 5 min idle after login.
set -eu
here=$(cd "$(dirname "$0")" && pwd); out=${OUT:-$PWD/out}; mkdir -p "$out"; cd "$out"
curl -fsSL -o arch.qcow2 https://geo.mirror.pkgbuild.com/images/latest/Arch-Linux-x86_64-cloudimg.qcow2
qemu-img resize -q arch.qcow2 16G
{
echo '#cloud-config'
echo 'write_files:'
SETUP=${SETUP:-setup}
for f in $SETUP measure trim; do
  echo "  - path: /root/$f.sh"; echo "    permissions: '0755'"; echo "    encoding: b64"
  echo "    content: $(base64 -w0 "$here/$f.sh")"
done
echo 'runcmd:'; echo "  - [bash, /root/$SETUP.sh]"
} > user-data
printf 'instance-id: measure\nlocal-hostname: measure\n' > meta-data
cloud-localds seed.img user-data meta-data
ACCEL=""; [ -w /dev/kvm ] && ACCEL="-accel kvm -cpu host"
run() { timeout "$2" qemu-system-x86_64 $ACCEL -m 4096 -smp 2 -display none -device virtio-vga \
  -drive file=arch.qcow2,if=virtio -drive file=seed.img,if=virtio,format=raw \
  -nic user,model=virtio -serial file:"$1" -monitor none || echo "boot $1 ended ($?)"; }
run setup.log 40m;   tail -3 setup.log
run measure.log 40m
sed -n '/=== MEASURE/,/=== END/p;/=== TRIM/,/=== TRIM END/p' measure.log | tee report.txt

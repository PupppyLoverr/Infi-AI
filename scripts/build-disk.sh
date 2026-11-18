#!/usr/bin/env bash
# Assemble a bootable raw disk image from the Solyra bootc base image.
#
# Works in environments where loop devices cannot be partitioned and vfat
# cannot mount (Amp orbs): partitions are accessed via losetup offsets, the
# ESP is written with mtools (userspace FAT), and `bootc install
# to-filesystem` performs the ostree deployment on the pre-mounted rootfs.
#
# Usage: sudo ./scripts/build-disk.sh [output.raw] [size]   (default 8G)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${SOLYRA_IMAGE:-solyra-base:dev}"
OUT="${1:-$REPO_ROOT/dist/solyra.raw}"
SIZE="${2:-8G}"
WORK="$(mktemp -d /tmp/solyra-disk.XXXXXX)"

log() { printf '\n== %s ==\n' "$*"; }

if [[ "$(id -u)" != 0 ]]; then
  echo "must run as root (sudo)" >&2
  exit 1
fi
for tool in losetup sfdisk mkfs.vfat mkfs.xfs mcopy grub-mkimage; do
  command -v "$tool" >/dev/null || { echo "$tool missing" >&2; exit 1; }
done

cleanup() {
  mountpoint -q "$WORK/root" 2>/dev/null && umount "$WORK/root" || true
  losetup -d "$ROOT_LOOP" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

log "creating $OUT ($SIZE)"
rm -rf "$OUT" "$OUT.meta"
mkdir -p "$(dirname "$OUT")"
truncate -s "$SIZE" "$OUT"

log "partitioning (ESP 512M + root xfs)"
sfdisk "$OUT" <<'EOF'
label: gpt
name=esp, size=512MiB, type=U
name=root, type=L
EOF
mapfile -t PARTINFO < <(sfdisk -d "$OUT" 2>/dev/null | sed -n 's/.* start= *\([0-9]*\), *size= *\([0-9]*\).*/\1 \2/p')
ESP_START="${PARTINFO[0]%% *}"; ESP_SIZE="${PARTINFO[0]##* }"
ROOT_START="${PARTINFO[1]%% *}"; ROOT_SIZE="${PARTINFO[1]##* }"
echo "esp: start=$ESP_START sectors=$ESP_SIZE | root: start=$ROOT_START sectors=$ROOT_SIZE"
ESP_OFFSET=$(( ESP_START * 512 ))
ROOT_OFFSET=$(( ROOT_START * 512 ))
printf 'esp=%s\nroot=%s\n' "$ESP_OFFSET" "$ROOT_OFFSET" > "$OUT.meta"

log "creating filesystems"
ESP_LOOP="$(losetup -f --show -o "$ESP_OFFSET" --sizelimit $(( ESP_SIZE * 512 )) "$OUT")"
ROOT_LOOP="$(losetup -f --show -o "$ROOT_OFFSET" --sizelimit $(( ROOT_SIZE * 512 )) "$OUT")"
mkfs.vfat -F32 -n EFI "$ESP_LOOP"
mkfs.xfs -L solyra -m reflink=1 "$ROOT_LOOP"

mkdir -p "$WORK/root"
mount "$ROOT_LOOP" "$WORK/root"

log "getting root UUID"
ROOT_UUID="$(xfs_admin -u "$ROOT_LOOP" 2>/dev/null | awk -F'= ' '{print $2}')"
echo "root uuid: $ROOT_UUID"

log "bootc install to-filesystem"
if [[ ! -f "${SOLYRA_OCI:-/tmp/solyra.oci}" ]]; then
  log "preparing OCI archive for bootc (skopeo)"
  docker save "$IMAGE" -o /tmp/solyra-docker.tar
  skopeo copy docker-archive:/tmp/solyra-docker.tar \
    oci-archive:"${SOLYRA_OCI:-/tmp/solyra.oci}":solyra-base:dev
  rm -f /tmp/solyra-docker.tar
fi
docker run --rm --privileged \
  -v "$WORK/root:/mnt/root" \
  -v "${SOLYRA_OCI:-/tmp/solyra.oci}:/mnt/solyra.oci:ro" \
  -e container=podman \
  --entrypoint /bin/bash \
  "$IMAGE" -c '
    set -euo pipefail
    # The ESP is a tmpfs mount point so bootc accepts the layout; the ESP
    # contents are copied onto the real ESP afterwards with mtools (vfat
    # cannot be mounted in the build environment).
    mkdir -p /mnt/root/boot/efi
    mount -t tmpfs tmpfs /mnt/root/boot/efi
    # The bootloader step fails here (build host has no EFI, and the root
    # loop device is not a real disk); the ostree deployment itself is
    # complete at that point and our standalone GRUB covers booting.
    bootc install to-filesystem \
      --source-imgref "oci-archive:/mnt/solyra.oci:solyra-base:dev" \
      --karg "root=UUID='"$ROOT_UUID"'" \
      /mnt/root || echo "bootc exited nonzero (bootloader step expected to fail)"
  '
echo "bootc install finished"

log "inspecting deployed boot layout"
find "$WORK/root/boot" -maxdepth 3 2>/dev/null | head -30
BLS="$(find -L "$WORK/root/boot" -name 'ostree-*.conf' 2>/dev/null | head -1 || true)"
if [[ -z "$BLS" ]]; then
  echo "no BLS entries found; bootc deploy may have failed" >&2
  exit 1
fi
echo "--- BLS entry: $(basename "$BLS")"
cat "$BLS"
BLS_LINUX="$(sed -n 's/^linux *//p' "$BLS")"
BLS_INITRD="$(sed -n 's/^initrd *//p' "$BLS")"
BLS_OPTIONS="$(sed -n 's/^options *//p' "$BLS")"

log "ensuring ostree boot-version links"
# bootc writes these in its finalize step, which is skipped when the
# bootloader step fails; ostree-prepare-root needs the karg path to resolve.
BLS_HASH="$(printf '%s' "$BLS_OPTIONS" | sed -n 's/.*ostree=\/ostree\/boot.[0-9]*\/[^/]*\/\([0-9a-f]*\)\/0.*/\1/p')"
DEPLOY_DIR="$(find -L "$WORK/root/ostree/deploy/default/deploy" -maxdepth 1 -name '*.0' -type d 2>/dev/null | head -1 || true)"
if [[ -n "$BLS_HASH" && -n "$DEPLOY_DIR" ]]; then
  DEPLOY_NAME="$(basename "$DEPLOY_DIR")"
  LINK_DIR="$WORK/root/ostree/boot.1/default/$BLS_HASH"
  mkdir -p "$LINK_DIR"
  if [[ ! -e "$LINK_DIR/0" ]]; then
    ln -s "../../../deploy/default/deploy/$DEPLOY_NAME" "$LINK_DIR/0"
    echo "created boot.1 link: default/$BLS_HASH/0 -> $DEPLOY_NAME"
  else
    echo "boot.1 link already present"
  fi
else
  echo "could not derive boot.1 link (hash='$BLS_HASH' deploy='$DEPLOY_DIR')" >&2
fi

log "installing GRUB standalone EFI into the ESP"
GRUB_CFG="$WORK/grub.cfg"
cat > "$GRUB_CFG" <<EOF
set default=0
set timeout=0
menuentry 'Solyra' {
  search --no-floppy --fs-uuid --set=root $ROOT_UUID
  linux $BLS_LINUX $BLS_OPTIONS console=ttyS0,115200
  initrd $BLS_INITRD
}
EOF
cat "$GRUB_CFG"
mkdir -p "$WORK/efiboot"
# No embedded config: prefix points at the ESP, so grub reads grub.cfg as a
# real file next to the binary; fat (ESP) and xfs (root) modules embedded.
grub-mkimage -O x86_64-efi \
  -o "$WORK/efiboot/BOOTX64.EFI" \
  -p '(hd0,gpt1)/EFI/BOOT' \
  part_gpt fat xfs search search_fs_uuid linux gzio echo boot
cp "$GRUB_CFG" "$WORK/efiboot/grub.cfg"

log "writing ESP via mtools (image@$ESP_OFFSET)"
# Build the ESP tree in a userspace FAT copy of the loop, then keep working
# directly against the raw image offset so writes land in the disk image.
mmd -i "$OUT@@$ESP_OFFSET" ::EFI ::EFI/BOOT
mcopy -i "$OUT@@$ESP_OFFSET" "$WORK/efiboot/BOOTX64.EFI" ::EFI/BOOT/BOOTX64.EFI
mcopy -i "$OUT@@$ESP_OFFSET" "$WORK/efiboot/grub.cfg" ::EFI/BOOT/grub.cfg
mdir -i "$OUT@@$ESP_OFFSET" ::/EFI/BOOT

log "syncing and detaching"
sync
umount "$WORK/root"
losetup -d "$ESP_LOOP" "$ROOT_LOOP"

log "measuring installed size"
du -sh --apparent-size "$WORK" 2>/dev/null || true
df -h "$OUT" | tail -1
echo "disk image ready: $OUT"
ls -lh "$OUT"

#!/usr/bin/env bash
# Assemble a bootable raw disk image from the Solyra bootc base image.
#
# Works in environments where loop devices cannot be partitioned and vfat
# cannot mount (Amp orbs): partitions are accessed via losetup offsets, the
# ESP is written with mtools (userspace FAT), and `bootc install
# to-filesystem` performs the ostree deployment on the pre-mounted rootfs.
#
# Usage: sudo ./scripts/build-disk.sh [output.raw] [size]   (default 8G)
# The result is an unsigned, x86_64 UEFI development disk, not installer media.

set -euo pipefail
export LC_ALL=C

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${SOLYRA_IMAGE:-solyra-base:dev}"
OUT="${1:-$REPO_ROOT/dist/solyra.raw}"
SIZE="${2:-8G}"
ESP_LOOP=
ROOT_LOOP=

log() { printf '\n== %s ==\n' "$*"; }

if [[ "$(id -u)" != 0 ]]; then
  echo "must run as root (sudo)" >&2
  exit 1
fi
for tool in losetup sfdisk mkfs.vfat mkfs.xfs xfs_admin mmd mcopy mdir grub-mkimage docker skopeo; do
  command -v "$tool" >/dev/null || { echo "$tool missing" >&2; exit 1; }
done

WORK="$(mktemp -d "${TMPDIR:-/var/tmp}/solyra-disk.XXXXXX")"
cleanup() {
  if mountpoint -q "$WORK/root"; then
    umount "$WORK/root" || return 1
  fi
  if [[ -n "$ROOT_LOOP" ]]; then losetup -d "$ROOT_LOOP"; fi
  if [[ -n "$ESP_LOOP" ]]; then losetup -d "$ESP_LOOP"; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

log "creating $OUT ($SIZE)"
mkdir -p "$(dirname "$OUT")"
if [[ -e "$OUT" || -L "$OUT" ]] || ! (set -o noclobber; : > "$OUT"); then
  echo "cannot reserve a new output file: $OUT" >&2
  exit 1
fi
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
OCI="${SOLYRA_OCI:-$WORK/solyra.oci}"
if [[ -z "${SOLYRA_OCI:-}" ]]; then
  log "preparing OCI archive for bootc (skopeo)"
  skopeo copy "docker-daemon:$IMAGE" "oci-archive:$OCI:solyra-base:dev"
fi
if [[ ! -f "$OCI" ]]; then
  echo "OCI archive not found: $OCI" >&2
  exit 1
fi
docker run --rm --privileged \
  -v "$WORK/root:/mnt/root" \
  -v "$(realpath "$OCI"):/mnt/solyra.oci:ro" \
  --entrypoint bootc "$IMAGE" install to-filesystem \
    --bootloader none --skip-finalize \
    --source-imgref "oci-archive:/mnt/solyra.oci:solyra-base:dev" \
    --root-mount-spec "UUID=$ROOT_UUID" /mnt/root
echo "bootc install finished"

log "inspecting deployed boot layout"
BLS="$(find "$WORK/root/boot" -name 'ostree-*.conf' -print -quit)"
if [[ -z "$BLS" ]]; then
  echo "no BLS entries found; bootc deploy may have failed" >&2
  exit 1
fi
echo "--- BLS entry: $(basename "$BLS")"
cat "$BLS"
BLS_LINUX="$(sed -n 's/^linux *//p' "$BLS")"
BLS_INITRD="$(sed -n 's/^initrd *//p' "$BLS")"
BLS_OPTIONS="$(sed -n 's/^options *//p' "$BLS")"

if [[ ! " $BLS_OPTIONS " =~ [[:space:]]ostree=(/ostree/[^[:space:]]+) ]] ||
   [[ ! -d "$WORK/root${BASH_REMATCH[1]}" ]]; then
  echo "BLS entry does not resolve to an OSTree deployment" >&2
  exit 1
fi

# Development-disk conveniences (VM testing only). The release image has no
# default password and no autologin (PLAN.md, P6 setup wizard).
#   SOLYRA_AUTOLOGIN=1      greetd logs the "solyra" user straight into the
#                           desktop (initial_session)
#   SOLYRA_DEV_PASSWORD=pw  give "solyra" this password on first boot, so
#                           sudo, the lock screen and a tty login work
DEPLOY="$(find "$WORK/root/ostree/deploy/default/deploy" -mindepth 1 -maxdepth 1 -type d -print -quit)"
VAR_DIR="$WORK/root/ostree/deploy/default/var"
if [[ "${SOLYRA_AUTOLOGIN:-0}" == "1" ]]; then
  # A development disk is already set up: skip the first-boot wizard.
  mkdir -p "$VAR_DIR/lib/solyra" && : > "$VAR_DIR/lib/solyra/setup-done"
  log "enabling development autologin (greetd initial_session)"
  mkdir -p "$DEPLOY/etc/greetd"
  cp "$DEPLOY/usr/etc/greetd/config.toml" "$DEPLOY/etc/greetd/config.toml" 2>/dev/null || cp "$REPO_ROOT/base/greetd.toml" "$DEPLOY/etc/greetd/config.toml"
  # The base config already has an [initial_session] (the setup wizard);
  # rewrite it to log the dev user straight in instead of adding a duplicate.
  sed -i 's|^command = "/usr/bin/solyra-first-login"$|command = "/usr/bin/solyra-session --session"|; s|^user = "solyra-setup"$|user = "solyra"|' "$DEPLOY/etc/greetd/config.toml"
  grep -q 'solyra-session --session' "$DEPLOY/etc/greetd/config.toml"
fi
if [[ -n "${SOLYRA_DEV_PASSWORD:-}" ]]; then
  log "setting a development password for solyra (first boot)"
  command -v openssl >/dev/null || { echo "openssl missing" >&2; exit 1; }
  HASH="$(openssl passwd -6 "$SOLYRA_DEV_PASSWORD")"
  # systemd expands $VAR in ExecStart, so every $ in the hash is doubled.
  HASH="${HASH//\$/\$\$}"
  UNIT_DIR="$DEPLOY/etc/systemd/system"
  mkdir -p "$UNIT_DIR/multi-user.target.wants"
  # The account is created by systemd-sysusers at boot, so the password is
  # applied by a oneshot after it. Only the hash is stored.
  cat > "$UNIT_DIR/solyra-dev-password.service" <<UNIT
[Unit]
Description=Solyra development disk: set the solyra account password
After=systemd-sysusers.service
Before=greetd.service systemd-user-sessions.service
ConditionPathExists=!/var/lib/solyra-dev-password.done

[Service]
Type=oneshot
ExecStart=/bin/sh -ec 'usermod -p '"'"'$HASH'"'"' solyra && touch /var/lib/solyra-dev-password.done'

[Install]
WantedBy=multi-user.target
UNIT
  ln -sf ../solyra-dev-password.service "$UNIT_DIR/multi-user.target.wants/solyra-dev-password.service"
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
  part_gpt fat xfs search search_fs_uuid linux gzio boot normal
cp "$GRUB_CFG" "$WORK/efiboot/grub.cfg"

log "writing ESP via mtools (image@$ESP_OFFSET)"
# Build the ESP tree in a userspace FAT copy of the loop, then keep working
# directly against the raw image offset so writes land in the disk image.
mmd -i "$OUT@@$ESP_OFFSET" ::EFI ::EFI/BOOT
mcopy -i "$OUT@@$ESP_OFFSET" "$WORK/efiboot/BOOTX64.EFI" ::EFI/BOOT/BOOTX64.EFI
mcopy -i "$OUT@@$ESP_OFFSET" "$WORK/efiboot/grub.cfg" ::EFI/BOOT/grub.cfg
mdir -i "$OUT@@$ESP_OFFSET" ::/EFI/BOOT

log "measuring installed size"
du -shx "$WORK/root"
log "syncing and detaching"
sync
cleanup
trap - EXIT
echo "disk image ready: $OUT"
ls -lh "$OUT"

<!-- SPDX-License-Identifier: GPL-2.0-only -->
# Running xnulinux as a hosted service

This manual covers what a hosted xnulinux service ships and what a user installs to run Apple's macOS on it. The
default system is PureDarwin, in which nothing is Apple's. A user who wants Apple's macOS adds it themselves, from
Apple's own downloads. Nothing of Apple's is shipped, stored or cached by the service, and complying with Apple's license
for what they install is the user's responsibility.

## 1. Two systems

| | **PureDarwin** (default) | **Apple's macOS** (the user adds it) |
|---|---|---|
| Kernel collection | `kc-free.bin`: our XNU, our kexts, PureDarwin's kexts built from source | `kc-apple.bin`: the same kernel and our kexts, plus Apple's kexts, linked on the user's machine |
| System disk | PureDarwin's userland on HFS+ (we build it) | made by Apple's installer from Apple's IPSW (the user makes it) |
| Licenses | APSL-2.0, BSD, MIT; Apache-2.0 for Mbed TLS (corecrypto) | Apple's, accepted by the user |

Both boot on the same `xnu.ko` and kernel, so a host can switch between them with `IMAGE=` (§4).

## 2. What we ship

Everything below is built from source in this repository and pinned upstreams. None of it is Apple's, and every
artifact comes with its source.

### 2.1 The host layer (Linux side)

| Artifact | From | License | Notes |
|---|---|---|---|
| `fki.ko` | `fki-ko/` | GPL-2.0-only | the foreign kernel interface: CPUs, memory carve-out, faults, IPIs, host calls |
| `xnu.ko` | `xnu-ko/` | APSL-2.0 | loads and runs the kernel collection. It is a non-GPL module that taints the kernel, like ZFS: ship its **source** and build it on the host against the running kernel (`xnulinux-boot.sh` does this on every boot, and a DKMS package would do it on kernel updates). Do not ship it prebuilt inside a kernel package |
| `xnudrmd` | `gpu/xnu-drm/xnudrmd.c` | GPL-2.0-only | the GPU bridge's Linux service (a virtio-gpu Venus render node, or another through virglrenderer: `xnudrmd_virgl.c`) |
| boot and service scripts | `scripts/xnulinux-boot.sh`, `install-service.sh`, `vm-install-fki-service.sh`, `vm-net-up.sh` | GPL-2.0-only | `xnulinux.service`, `xnulinux-fki.service`, `/etc/xnulinux.conf` |
| kernel command line | `vm-install-fki-service.sh` | | `mem=`, `nokaslr`, `log_buf_len=`; the carve-out (`carve_base`, `carve_mb`) sized for each host model |
| on Mac hosts | `scripts/krun-vm.sh`, `install-krun-agent.sh`, krunkit (Apache-2.0) | | the Linux VM with virtio-gpu Venus; a launchd agent keeps it running |

The host Linux is a stock arm64 distribution kernel (Ubuntu 24.04 is tested) with 4K pages. The CPU needs PAC
(`FEAT_PAuth`), and Linux must run at EL1, not as a bare-metal VHE kernel at EL2 (`kvm-arm.mode=nvhe` on a DGX Spark).
`fki.ko` refuses to load otherwise.

### 2.2 The kernel kit

`scripts/apple-kc/package.sh KIT` (after `scripts/vm-xnu-rebuild.sh`) writes it. It is about 95 MB:

| File | What |
|---|---|
| `kernel.macho` | XNU `xnu-12377.121.6` with `xnu-patches/`, built from source (APSL-2.0). It links no Apple binaries: compiler-rt's kext-PGO functions are stubbed. |
| `kexts/NAME/` | our kexts as arm64e objects plus the description `kclink` links them from (`XnuCPU`, `XnuDisk`, `XnuDrm`, display, input, ethernet...) |
| `kc-free.bin` | the PureDarwin kernel collection (`scripts/free-kc/build.sh`). It is the same byte for byte whether or not any Apple kernelcache is visible to the build. |
| `kit.json` | for `link.sh`: the sha256 of the one kernelcache it links against and Apple's IPSW URL for it, the Apple kexts to keep (`scripts/apple-kc/keep.txt`), the pointers to rebind, AMFI's platform version |
| `scripts/kclink`, `scripts/apple-kc`, `scripts/kc-amfi-platform.py` | the linker and the user-side scripts (GPL-2.0-only), shipped with the kit but not in it (`package.sh` writes the four files above): unpacked as `/opt/xnulinux/scripts` (§4) |

The kernel's segment slots are laid out for one macOS build (today 27.2, `26B123`, `kernelcache.release.vma2`). Each
macOS build we support gets its own kit, and `link.sh` refuses any other kernelcache.

### 2.3 The PureDarwin system disk

- `scripts/puredarwin-root.sh` takes the root file system from PureDarwin's `image-arm64-virt-minimal`, and
  `scripts/mk-puredarwin-disk.sh` makes the HFS+ disk and its `MAC_CONF`. Pin the PureDarwin revision, and ship its
  source references with the disk.
- PureDarwin's kexts in `kc-free.bin` (`IOStorageFamily`, `HFSEncodings`, `hfs`, `pthread`, the AMFI stub) are built
  from source as arm64e with this kernel's flags (`docs/free-kc.md`); its corecrypto is xnulinux's, on Mbed TLS
  (`docs/corecrypto.md`).
- PureDarwin's userland still carries PureDarwin's interim `libcorecrypto` (unaudited, by its own README) on its disk.

### 2.4 The GPU parts for macOS users

We ship these. The user's disk script installs them into their own macOS disk:

| Artifact | License |
|---|---|
| `gpu/xnu-cpu-metal` (the Metal plugin `XnuCpuMetal.bundle`: metal2vulkan, lavapipe) | GPL-2.0-or-later |
| the Darwin Venus driver (`gpu/venus/build-darwin.sh`) and `gpu/xnu-drm/darwin` (libxnudrm) | MIT |

### 2.5 What we never ship, store or cache

- Apple's kernelcache, or any kernel collection made from it (`kc-apple.bin`, `kc.bin`)
- macOS system disks, IPSWs or installers, or anything taken from them: cryptexes, the static trust cache
  (`trustcache-mac.bin`), keybags
- screenshots of macOS for marketing. `docs/images/` shows macOS for documentation, and Apple's interface and
  wallpaper in them are Apple's.

## 3. Operator: preparing a host

1. **Host OS.** arm64 Ubuntu, either directly on the hardware or as a VM. On an Apple Silicon Mac the VM runs under
   krunkit (krunkit, libkrun, virglrenderer, MoltenVK and gvproxy under `$KRUN`; `scripts/krun-vm.sh`,
   `scripts/install-krun-agent.sh`), which gives it the host GPU through virtio-gpu Venus.
2. **Memory.** `scripts/vm-install-fki-service.sh` with `LINUX_MEM`, `CARVE_BASE` and `CARVE_MB` for that host's
   physical memory map. The carve-out is RAM Linux never manages. The hosted kernel's `RAM_MB` has to fit inside it
   (8 GB → `RAM_MB=7900`). Reboot once.
3. **Kit.** Unpack it to `/opt/xnulinux/kit`, and install `kc-free.bin` as `/lib/firmware/xnu/kc-free.bin`.
4. **Default system.** Put the PureDarwin disk at `/var/lib/xnulinux/pd.img` with its `pd.conf`. Configure
   `/etc/xnulinux.conf` as follows, then run `scripts/install-service.sh`:
   ```
   IMAGE=xnu/kc-free.bin
   TRUSTCACHE=
   MAC_CONF=/var/lib/xnulinux/pd.conf
   DISK=/var/lib/xnulinux/pd.img
   GPU=off
   CPUS=2
   RAM_MB=7900
   ```
   Run it as the user the service is to run as. The service has no terminal, so that user needs sudo without a
   password (a `NOPASSWD` rule), or run the installer as root.
5. **Isolation.** The hosted kernel runs at EL1 inside the Linux kernel, so a hosted system that is compromised owns
   that Linux. **The tenant boundary is the VM (or the machine), never the Linux.** Run one Linux VM per tenant, with one
   hosted system in it. Never run two users' hosted systems in one Linux.

## 4. Running Apple's macOS

The user runs these steps themselves: they download Apple's files and accept Apple's license. They need python3, `lzfse`
(`apt install lzfse`) and network access to Apple's CDN on the Linux host, and a Mac to make the system disk with
Apple's installer (`tart` or another Virtualization.framework installer).

1. **Kernel collection: link Apple's kexts into the shipped kit.** On the Linux host:
   ```
   /opt/xnulinux/scripts/apple-kc/link.sh /opt/xnulinux/kit
   ```
   It downloads only `kernelcache.release.vma2` from Apple's macOS 27.2 IPSW (HTTP range requests, about 81 MB, not the
   whole IPSW) and checks its sha256 against `kit.json`. It then links the Apple kexts the kit names (31, 72 with their dependencies) with our kernel
   and kexts using `scripts/kclink`, and installs the result as `/lib/firmware/xnu/kc-apple.bin`. A local IPSW, its URL,
   the IM4P or an unpacked kernelcache can be given as a second argument instead. The output is the same byte for byte
   on every machine.
2. **System disk: Apple's installer, on a Mac.** Install macOS from Apple's IPSW into a Virtualization.framework
   disk (`tart create NAME --from-ipsw …`), then run `SRC=~/.tart/vms/NAME/disk.img scripts/mk-macvm-disk.sh` (it works on an APFS clone of it). It merges the
   OS cryptex into the system volume (`docs/production.md` §3), adds to the Data volume: the local user and automatic login, the Metal plugin (§2.4), the
   console shell, and a list of disabled hardware daemons. It also takes the static trust cache from the disk's own
   Preboot volume. It sends `mac.img`, `mac.conf` and `trustcache-mac.bin` to the Linux host.
3. **Boot it.** Edit `/etc/xnulinux.conf`, then run `sudo systemctl restart xnulinux`:
   ```
   IMAGE=xnu/kc-apple.bin
   TRUSTCACHE=xnu/trustcache-mac.bin
   MAC_CONF=/var/tmp/mac.conf
   DISK=/var/tmp/mac.img
   GPU=auto
   ```
   Switching back to PureDarwin is `IMAGE=xnu/kc-free.bin` with its disk. Both collections share the same kernel and
   `xnu.ko`.
4. **macOS updates.** The user's disk and kernel collection must come from the builds the kit supports. When we
   ship a kit for a new macOS build, the user reruns step 1 (`link.sh` with the new kit) and step 2. Until then,
   macOS's Software Update must stay off: an updated disk will not match the kernel collection. If the disk's build is
   older than the kernelcache's, set `AMFI_PLATFORM` (default 26; `scripts/kc-amfi-platform.py`) when running
   `link.sh`.

## 5. Not ready yet

| Gap | Effect on the service |
|---|---|
| PureDarwin's userland `libcorecrypto` is PureDarwin's unaudited interim fork (§2.3) | replace it on the PureDarwin disk before offering it for anything that keeps secrets |
| Making the macOS disk needs a Mac with Xcode and `tart` (`mk-macvm-disk.sh`) | users need a Mac to make their macOS disk; a Linux disk maker (from the IPSW's APFS images) would remove that. Check `tart`'s own license for commercial use, or use another Virtualization.framework installer. The PureDarwin disk needs none: `mk-puredarwin-disk.sh` makes it on the Linux host (`mkfs.hfsplus`) as well |
| One kit per macOS build, fixed segment slots | each macOS release needs a new kit before users can update |
| AMFI relaxed (`amfi_get_out_of_my_way=1`) and SIP partly off (`lp-sip0`); the OS cryptex merged into the system volume | the hosted macOS is less locked down than a real Mac. State this in the service's documentation (`docs/production.md` §3) |
| The keystore is fresh at every boot | FileVault-style data protection does not persist across reboots |
| GPU bridge: virtio-gpu (Venus), or virglrenderer on the host's own Vulkan driver (tested on a DGX Spark) | Mac hosts and the DGX Spark get the host GPU; other bare-metal hosts are untested |
| Bare-metal hosts need their own carve-out layout (`mem=`, `carve_base`) | one layout per host model, set up when it is provisioned |
| `xnu.ko` built on the host at boot | needs kernel headers and a compiler on the host. Package it as DKMS |

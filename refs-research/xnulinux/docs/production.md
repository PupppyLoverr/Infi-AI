# From PoC to production

What the PoC did to get each step running, and what replaces it. Ordered by what it costs if left in place.

## 1. XNU patches

| Now | Replace with |
|---|---|
| Done: four topic patches (`xnu-patches/000{1..4}-linux64-*.patch`: machine layer, host devices, boot without iBoot, macOS 27 userland), each file in one of them | Finer topics as they change (el0, pmap, timer within the machine layer) |
| Done: one header pair (`xnu-patches/include/linux64.h`, `linux64_abi.h`, copied into `EXTERNAL_HEADERS/linux64`); calls through `linux64_call()`; `if (linux64_hook)` as the hosted test | Code under `CONFIG_LINUX64` so a non-hosted build is the stock kernel |
| Done: bring-up stage markers, the dead second idle path, the kernel's own en0, unused page-table walkers and probes removed; the idle-loop thread dump behind `LINUX64_DEBUG`; system call and exec path logging only with boot-arg `linux64_trace`; monitor counters named (`enum linux64_counter`) | A thread dump on demand (a call from the host) instead of on an idle counter |
| Done: the PoC's non-cryptographic xorshift fallback for `read_random`/`cc_rand_generate` before corecrypto registers | The host's generator (`LINUX64_CALL_RANDOM`: Linux `get_random_bytes`) |
| memdev host mode `hostdisk=` (replaced by `kexts/XnuDisk`), HFS+ read-write root | Removed |

## 2. The xnu.ko ↔ XNU boundary

| Now | Replace with |
|---|---|
| Done: `linux64_abi.h` shared by both sides: named calls (`enum linux64_call`), one request struct per call, `LINUX64_ABI_VERSION`; xnu.ko logs a call it does not know and goes on | Check the version at load (XNU exports it) |
| Console input, input events, framebuffer: host files appended to or dumped on a timer (`console_in=`, `input_in=`, `fb_dump=`) | Character devices from fki.ko (`/dev/xnu/console`, `/dev/xnu/input`, `/dev/xnu/fb` with mmap), so a host program talks to them like any device |
| Module parameters from bring-up (`mode`, `kmax`, `idle_max`, `ramdisk`, `hostdisk`; `stop` is gone, `kmax`/`idle_max` default to no limit) | Drop the rest; the run ends when XNU halts |

## 3. Boot and the system disk

| Now | Replace with |
|---|---|
| `mk-macvm-disk.sh` merges the OS cryptex into the sealed System volume (breaks the seal) | Graft the OS and App cryptexes at boot as macOS does (`graftdmg`), so the System volume stays sealed and an OS update replaces only the cryptex |
| `kc-amfi-platform.py` patches AMFI's expected platform version in the kernel collection | Build the collection from the same macOS build as the disk (one version source), no byte patching |
| `amfi_get_out_of_my_way=1`, `lp-sip0=0x802`, `kern.memorystatus_highwater_enabled=0` | Keep SIP/AMFI on; sign our plugin and kexts (ad-hoc trust cache of our own), only the unauthenticated-root bit stays |
| `/chosen` UUIDs passed by hand (`root_uuid=`, `preboot_uuid=`) | Read from the disk's APFS container at load (xnu.ko or a host tool), no manual parameters (step one done: `mk-macvm-disk.sh` writes them to `mac.conf`, `xnulinux-boot.sh` reads it) |
| Fixed `carve_base` (`0x200000000` in a 16 GB VM) with `nokaslr` so Linux's arm64 EFI stub does not place the kernel image inside the carve-out before `mem=` takes effect | Have `fki` choose the carve-out dynamically at load from the actual memory map (avoiding the kernel image and reserved regions), then re-enable KASLR |

## 4. Scripts and tests

| Now | Replace with |
|---|---|
| ~30 scripts; the boot runs from `xnulinux-boot.sh` (builds, checks, retries, GPU bridge; the bisecting `vm-m2-kbisect.sh` is gone); several have fixed `sleep`s and host-specific defaults | One driver (`xnulinux` CLI): `build`, `disk`, `boot`, `console`, `screenshot`, `test`; configuration in one file |
| HFS+ route scripts (`mk-fulldisk.sh`, `vm-sysdisk-*`, `vm-mk-sysdisk.sh`, `vm-m2-*`, `run-poc.sh`) | Removed (done: APFS route only) |
| `tools/hosted/tests/t*.sh`, `af*.sh`: one-off probes (removed) | A test suite with expected results: boot to desktop, Safari/Chrome page load (screenshot compare), `xcodebuild` of a pinned project, Metal unit tests |
| Done: CI (`.github/workflows/modules.yml`) builds fki.ko and xnu.ko on arm64 Ubuntu against the newest generic kernel headers and checks every source file for an SPDX line; `xnu-ko-loader.yml` runs xnu.ko's loader in user space under AddressSanitizer on images the test makes (`xnu-ko/test/run.sh`) | Also: XNU + collection build (x86_64 Linux works with the DroidVM toolchain), GPU plugin host tests on macOS |

## 5. GPU

| Now | Replace with |
|---|---|
| CPU only: Metal → metal2vulkan → lavapipe in every process (WindowServer's GPU work runs on the CPU) | Hardware: the same plugin over Mesa's Vulkan driver for the host's GPU, through a DRM bridge (below) |

The DRM bridge: a Mesa Vulkan driver built for Darwin runs in the macOS process; its DRM calls go to an IOKit user client
(`kexts/XnuDrm`) and through FKI to the Linux DRM device. Buffer objects are Linux pages mapped straight into the macOS task
(same physical memory, no copies). On an Apple Silicon host the Linux DRM device is virtio-gpu with Venus (krunkit:
Venus → MoltenVK → Metal on the host GPU). On a Linux machine with its own GPU there is no virtio-gpu: `xnudrmd` is the
virtio-gpu device itself, serving the same client from virglrenderer's Venus on the machine's Vulkan driver
(`gpu/xnu-drm/xnudrmd_virgl.c`). On a DGX Spark the hosted macOS gets `Virtio-GPU Venus (NVIDIA GB10)`: the compute
test and a Metal compute kernel read back correct results and WindowServer composites through it (not yet measured).

### Status of the bridge

- Linux under krunkit (`scripts/krun-vm.sh`) sees the host GPU through Venus: `Virtio-GPU Venus (Apple M5 Max)`, and
  `gpu/venus/test/compute.c` (a compute dispatch over 16 MB of host-visible memory) reads back correct results. Mesa needs
  16 KiB blob alignment for a macOS host (`gpu/venus/patches/0001`). Mesa 25.x also refused a renderer without
  `VK_KHR_external_memory_fd` (virglrenderer on macOS shares mappings in-process instead of exporting fds); 26.x no longer
  checks.
- The bridge works end to end: in the hosted macOS, `gpu/venus/test/compute.c` built for macOS loads the Darwin Venus
  driver (`gpu/venus/build-darwin.sh`), sees `Virtio-GPU Venus (Apple M5 Max)` and its compute dispatch over 16 MB of
  shared memory reads back correct results (6.7 ms). Pieces: `fki-ko/fki_hostcall.c` (`/dev/xnu-hostcall`), xnu.ko
  upcall 830, `kexts/XnuDrm` (user client; maps Linux's pages into the task), `gpu/xnu-drm/xnudrmd.c` (Linux service on
  the render node), `gpu/xnu-drm/darwin` (libxnudrm, compiled into Mesa by `gpu/venus/patches/0002`).
- Metal on it: the plugin (`gpu/xnu-cpu-metal`) carries both drivers and takes Venus when `XnuDrm` is there; a Metal
  program in the hosted macOS gets `Virtio-GPU Venus (Apple M5 Max)` (textures, IOSurface textures, buffers, blits).
- WindowServer composites on the host GPU: the plugin hands the loader the chosen driver directly
  (`VK_LUNARG_direct_driver_loading`, lavapipe is not loaded with Venus), the bridge's user client sits on the Metal
  accelerator service (the sandbox's `system-graphics` rule), and `newBufferWithBytesNoCopy` buffers, copied because
  Venus has no `VK_EXT_external_memory_host`, are synced from the caller's memory before each submission.
- Open: those copies (up to the 33 MB wallpaper per submission) and the IOSurface copies to and from GPU memory each
  frame (2-5 ms at 1080p-1440p, measured on the host): host-memory import and IOSurfaces backed by bridge blobs.

## 6. Performance

Measured against the same macOS 27.2.1 install booted as an Apple Virtualization.framework VM (tart, 2 CPUs) on the same
host, back to back; time from `kern.boottime` to the Dock process starting (first login, automatic):

| | loginwindow | Dock |
|---|---|---|
| Native VM, 2 CPUs | +3 s | +49 s |
| Hosted, 1 CPU | +11 s | +32 s |
| Hosted, 2 CPUs | +11 s | +31 s |

What it took:

- The machine identity (`xnu.ko model=`, default `VirtualMac2,1`): sandboxed processes read MobileGestalt's product type
  from `/product`'s `sub-product-type` and `unique-model` only; without them apsd aborted at every start and loginwindow's
  MDM pre-login check-in (`CP_PreLoginCheckIn`) waited about 100 s for it.
- The user-space exception entry stores the user state and signs it once (it checked and re-signed it five times).

Per operation (hosted / native VM): getppid 270 / 87 ns, write 64 B 533 / 651 ns, pipe round trip 3.3 / 8.7 µs,
fork+exit+wait 0.97 / 0.25 ms, posix_spawn+wait 3.0 / 1.0 ms. Memory access with 4 KB pages: a 4 MB pointer chase
27–37 / 18 ns (the hardware translation granule is the host's; Linux in the same VM measures the same).

Tools: `xnu.ko perf=1` prints, every 2 s, each host call's share of the time and the user-space exceptions by class
(system call, data and instruction abort, IRQ) with their time until the CPU is back in user space; XNU's `pc-sample=`
records, hosted, the caller where interrupts are re-enabled for kernel samples.

| Now | Replace with |
|---|---|
| Done: 16 KB VM pages by default (`xnu.ko vm16k=1`, over the 4 KB granule, XNU's PAGE_RATIO 4): a quarter of the page faults, the desktop ~30% sooner, fork 2-3x faster than 4 KB; the WindowServer abort it seemed to cause was a start-up race with `xnudrmd` (fixed), and the early hang seen once did not come back in ~30 boots | The host's own 16 KB granule (a 16 KB Linux kernel) for the TLB reach native macOS has: memory-bound code is still up to 1.6x slower |
| The system disk keeps hardware daemons that crash or retry without their hardware | No crash reports after a boot: each crashing daemon fixed (a stand-in service or property) or disabled with a reason |

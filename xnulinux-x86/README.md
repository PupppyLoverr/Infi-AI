# xnulinux on x86_64 — working recipe (unmodified xnulinux)

Proof: XNU 25.5.0 (macOS 27 kernel) executes inside Linux on x86_64 hardware.
mode=1 smoke test passes (XNU computes strlen+crc32 of its own version string);
mode=2 boots into arm_init, XNU kprintf runs (cache geometry, processor_init),
then XNU panics in early platform bring-up — the same ceiling upstream's PoC
has on unsupported hardware. mode=3 (full macOS) needs a Mac-made disk, as ever.

## Architecture
- QEMU full-system emulation of arm64: qemu-system-aarch64 -M virt -cpu max,pauth=on
  -smp 6 -m 16G, Ubuntu noble arm64 cloudimg (kernel 6.8.0-147-generic, 4K pages).
  TCG emulates FEAT_PAuth: in-guest /proc/cpuinfo Features shows `paca pacg`.
- XNU kernel (kernel-k27.macho, with xnu-patches applied) cross-built natively on
  x86_64 docker (ubuntu:24.04 + LLVM 21 + macOS-DroidVM toolchain) — upstream
  confirms x86_64 build hosts work.
- fki.ko / xnu.ko cross-compiled in docker against extracted Ubuntu arm64
  linux-headers 6.8.0-147 + gcc-aarch64-linux-gnu.
- xnulinux sources UNMODIFIED (pristine clone). All compat is external:
  - w/neon-shim.h (force-included): drops vestigial &scratch arg of
    kernel_neon_begin/end; emulates follow_pfnmap_start/end (>=6.10 API)
    with follow_pte for 6.8.
  - module.lds in the headers tree: arm64 requires .plt/.init.plt/.text.ftrace_trampoline
    sections (BYTE(0) entries); UBUNTU's patched loader additionally REQUIRES
    .init.text.ftrace_trampoline — missing => silent -ENOEXEC at insmod.
    ALSO: scripts/ host binaries (fixdep etc) in arm64 headers are aarch64
    binaries — overlay the amd64 headers package's scripts/ for cross builds,
    but restore/keep the arm64 module.lds afterwards.

## VM bring-up
1. run-vm.sh (uefi pflash, hostfwd ssh :10022, -serial mon:stdio log)
2. in VM: grub.d/99-xnulinux.cfg adds `mem=7G nokaslr log_buf_len=128M`,
   update-grub, reboot (~8 min under TCG)
3. insmod fki.ko carve_base=0x200000000 carve_mb=8156
   -> "fki: 8156 MiB at 0x200000000 for the foreign kernel's DRAM"
4. cp kernel.macho /lib/firmware/xnu/kernel.macho
5. insmod xnu.ko image=xnu/kernel.macho mode=1  -> smoke test PASS
6. insmod xnu.ko image=xnu/kernel.macho mode=2  -> XNU boot, panic in
   early processor/platform init (expected: QEMU virt is not Apple/RPI hardware)

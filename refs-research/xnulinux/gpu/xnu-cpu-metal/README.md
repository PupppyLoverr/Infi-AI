# XnuCpuMetal: a CPU Metal device for the hosted macOS

`XnuCpuMetal.bundle` is a Metal plugin (`/Library/GPUBundles`) whose device runs in the calling process:
Metal API → `XnuCpuMetalDevice` (an `IOGPUMetalDevice` subclass) → metal2vulkan (AIR → SPIR-V, Metal runtime on Vulkan) →
Vulkan loader → Mesa lavapipe (llvmpipe JIT on the CPU). The kernel side is `kexts/XnuCpuMetal` (an `IOAccelerator` naming the plugin).
It gives the hosted macOS a working `MTLCreateSystemDefaultDevice()` with no GPU and no window server: enough for Xcode's
build-time renderers (`ictool`/Icon Composer, `actool`, CoreImage).

## What is here (ours) and what is fetched
- `src/xnu-cpu-metal-bundle/`: the plugin (Objective-C++); `XnuCpuMetalShim.h` adapts UniXNU/AdrenoMetal's shim (BSD-2-Clause-Patent).
- `src/xnu-m2v-bridge/`: the Rust C-ABI bridge over metal2vulkan (`libxnu_m2v.dylib`).
- `scripts/`: builds (arm64 and arm64e), host-side tests.
- `scripts/fetch-sources.sh` fetches, at pinned revisions and **never committed**: metal2vulkan (GPL-2.0-or-later / LGPL-3.0-or-later),
  Vulkan-Loader (Apache-2.0), Mesa 26.3.1 (lavapipe, MIT), LLVM 22.1.8. None of it goes into XNU or the Linux kernel.

## Build (on an Apple silicon Mac with Xcode, Homebrew and rustup)
Once, the tools (what `.github/workflows/metal-plugin.yml` installs):

    brew install meson ninja cmake pkgconf bison flex glslang zstd python3 llvm@22
    brew unlink spirv-tools             # it comes with glslang; Mesa must not link Homebrew's SPIRV-Tools into the bundle
    rustup toolchain install 1.100.0 --profile minimal
    rustup toolchain install nightly-2026-11-03-aarch64-apple-darwin --profile minimal --component rust-src

Then, in this order (each step uses what the ones before it left under `build/` and `dist/`):

    scripts/fetch-sources.sh
    scripts/setup-deps.sh                                           # build/pyvenv, build/vk-deps: Python, bison, flex, Vulkan headers
    scripts/build-vkloader-arm64e.sh                                # the Vulkan loader, arm64 and arm64e
    scripts/build-lavapipe.sh && scripts/build-m2v.sh               # arm64
    scripts/build-llvm-arm64e.sh                                    # LLVM for the arm64e llvmpipe JIT: about 40 minutes
    scripts/build-lavapipe-arm64e.sh && scripts/build-m2v-arm64e.sh # arm64e
    ../venus/build-darwin.sh                                        # optional: the Venus driver, taken into the bundle when built
    scripts/build-bundle-universal.sh                               # -> dist/XnuCpuMetal.bundle
Install: `scripts/mk-macvm-disk.sh` (repo root) puts the bundle on the system disk; the hosted kernel needs the boot-arg `lp-sip0=2`.

## Hosted-system findings (what Metal requires of a plugin, beyond the host tests)
- The accelerator service must not carry `Removable` / `SafeEjectRequested` at all (Metal skips a service on which either exists).
- `/Library/GPUBundles` is trusted only with `CSR_ALLOW_UNRESTRICTED_FS` (boot-arg `lp-sip0=2` in the hosted kernel).
- The device class must conform to `MTLDeviceSPI` (`MTLAddDevice` asserts it).
- `targetDeviceArchitecture` must be an `MTLTargetDeviceArchitecture` (the AIR type here), or binary archives assert.

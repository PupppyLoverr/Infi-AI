# GPUI support — investigation findings

Status: **investigated, not shipped.** No claim of GPUI support is made
anywhere in CosmosOS — this doc records what was tried and what it would
take.

## What GPUI is

GPUI is Zed's GPU-accelerated UI framework (zed-industries/zed). On Linux
it renders through Blade (its Vulkan/wgpu-style backend) and uses either
XCB or Wayland for windowing. It is not a general-purpose supported
crate; it is a workspace crate extracted from Zed and published with
limited platform coverage.

## What was tried

A minimal `gpui` window app (the `Application::new().run(|cx| ...)` +
`div().child("Hello")` pattern) against `gpui = "0.2.2"` from crates.io,
on the project's aarch64 Debian build host.

Result: **dependency tree does not compile on Linux aarch64.**
`xattr` (pulled transitively by gpui 0.2.2) fails to build:

```
error[E0425]: cannot find value `ENOATTR` in this scope
   (xattr's platforms! macro uses ::libc::ENOATTR)
```

`ENOATTR` is a BSD/macOS errno; glibc has `ENODATA` instead — the
published dep tree carries the macOS-oriented resolution. The Linux-capable
GPUI lives in the zed monorepo (`gpui` via `git = zed-industries/zed`),
which additionally requires: Vulkan-capable graphics (Blade → real GPU or
`lavapipe`/virtio-gpu), font stack (cosmic-text/fontkit), and a much
heavier dependency tree (weezl, tower, async-std, http clients — several
hundred crates; the spike compiled ~250 crates before failing).

## Compatibility verdict for CosmosOS

A Zed-repo GPUI client built for Wayland *should* work under
cosmos-compositor: we implement `wl_compositor`, `xdg_wm_base`, shm and
(when a GPU is present) dmabuf; gpui's Wayland platform needs exactly
those plus keyboard/pointer seats, which we provide. Its Vulkan path
could run on lavapipe (llvmpipe's Vulkan driver) in QEMU or real GPUs on
metal — no compositor changes needed.

Recommendation: keep **egui (cosmos-uitk)** as the stock app toolkit —
same Rust, a fraction of the dependency weight, software-rendered so it
works on every target including no-GPU QEMU. GPUI remains a viable path
for a future heavyweight native app, but it should be adopted by building
a real app against the zed-repo crate — not claimed until one actually
renders under our compositor.

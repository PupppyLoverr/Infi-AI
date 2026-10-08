# Lift catalog — pop-os/cosmic-comp as read-only reference

The fork is cancelled. Our compositor stays. cosmic-comp (GPL-3.0,
Rust + Smithay, upstream pin: smithay git `e3d461a`, RON cosmic-config,
~33.8MB release binary) is kept as a **read-only parts catalog** — study and
lift specific pieces rather than re-derive them. Any copied code keeps its
GPL-3.0 headers + upstream copyright; nothing user-visible may say
COSMIC/Pop!_OS/System76.

Recon facts (x86_64 build host, Oct 2026): clean 6m02s release build with
rust 1.93 / edition 2024; only added pkg was `xwayland`; SSD for all clients;
server-side wlr-layer-shell; autotile Global/PerWorkspace, workspaces
OutputBound/Global; ~128MiB idle nested; X11 backend needs DRI3 (winit
fallback works).

## What to lift, in priority order

1. **XWayland** — how cosmic-comp lazily starts XWayland, wires the WM
   selection, manages X11→Wayland focus/activation, and maps override-redirects.
   Reference: `src/xwayland_handler.rs`, `src/backend/x11/`, shell x11 code.
2. **Screencopy / portal plumbing** — `wlr-screencopy-unstable-v1` +
   `ext-image-copy-capture` handling and how frames get DMA-buf'd or shm'd out,
   so our xdg-desktop-portal backend can screencast/screenshot.
   Reference: `src/wayland/handlers/screencopy*`, portal bits under `src/`.
3. **Fractional scaling** — surface-space vs physical-space conventions,
   `wp_fractional_scale_v1` + buffer_scale interplay, and damage in
   fractional space.
4. **Multi-monitor hotplug** — output add/remove, mode lists, workspace
   migration on disconnect, EDID→mode selection.
5. **Anything cheap**: pointer-constraints/relative-pointer confinement,
   keyboard-inhibit for lockscreens, idle-inhibit semantics, text-input +
   IME plumbing — check ours vs theirs before writing our own.

## How to lift responsibly

- Prefer *reading* for approach, then writing our own code (same Smithay
  APIs, different structure) — copy only where the logic is protocol-mandated.
- Keep GPL-3.0 headers verbatim on any copied file/hunk; note provenance in
  the commit message (`lifted from pop-os/cosmic-comp@<sha> <path>`).
- `upstream` remote stays configured but is never merged wholesale —
  cherry-pick deliberately, review each hunk.
- Never adopt its config surface wholesale (RON cosmic-config) or its
  decoration/tile UX — ours is spec'd in docs/ui-ux.md.

## Status of our own surface (post-audit, unchanged)

Our IPC (requests/events), keybind set, snap+assist+groups, master+stack
tiling, minimize/show-desktop, motion, SSD chrome, accent presets, shm
correctness on llvmpipe, and the layer-shell contract the shell depends on —
all stay exactly as verified. The fork cancellation deleted the port list;
nothing else about the shell changes.

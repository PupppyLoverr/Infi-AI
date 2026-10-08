# CosmosOS — Architecture

Strategy: **borrow the plumbing, own the experience.** Proven open-source
parts for the invisible layers; everything the user sees and touches is ours.

## Layers

```
┌─────────────────────────────────────────────────────────────┐
│ cosmos-shell (one process)                                  │
│ menubar · dynamic island · dock (left edge) · Search-or-Ask │
│ Start panel+widgets · Control Centre · notifications · lock │
│  — layer-shell surfaces, our design system                  │
├─────────────────────────────────────────────────────────────┤
│ cosmos-compositor — OUR compositor (Rust + Smithay 0.7)     │
│ floating + master+stack tiling + workspaces + snap/assist + │
│ minimize + motion + SSD chrome + cosmos-ipc                 │
├─────────────────────────────────────────────────────────────┤
│ cosmos-agentd — agent runtime + MCP tools (desktop, a11y,   │
│ files, system, notify, clipboard, island)                   │
├─────────────────────────────────────────────────────────────┤
│ Debian 13 trixie — GPT + btrfs subvolumes + snapper, zram,  │
│ systemd-oomd, PipeWire/WirePlumber, NetworkManager, BlueZ,  │
│ flatpak, xdg-desktop-portal, Calamares installer            │
└─────────────────────────────────────────────────────────────┘
```

## The compositor decision (post-audit)

**cosmos-compositor stays.** It is already Smithay 0.7 with ~13.5k LOC and
working snap/assist/tiling/motion/IPC, verified through 10+ guest drive
rounds at ~330–410 MiB idle. Forking pop-os/cosmic-comp would trade that
correctness for a re-integration port — rejected after the Phase 1 audit.

`pop-os/cosmic-comp` (GPL-3.0) remains a **read-only reference**: specific
pieces get lifted into our compositor where it's cheaper than re-deriving
them — XWayland integration, xdg-desktop-portal/screencopy plumbing,
fractional scaling, multi-monitor hotplug. Any copied code keeps its GPL-3.0
headers; no COSMIC/Pop!_OS/System76 user-visible branding anywhere.

## Shell rendering: egui, tiered

The shell is **egui on cosmos-uitk's software rasterizer** — deliberately,
not as a placeholder. Do not rewrite to iced.

- **Lite tier** — the current per-pixel barycentric rasterizer (works on
  llvmpipe/TCG everywhere).
- **Balanced/Ultra tiers** — a GPU path via `egui-wgpu` (or `glow` shim) behind
  a render-backend trait, selected at runtime by detected capability. GPU
  compositing is what unlocks live backdrop blur on Balanced/Ultra.

## Why Debian over Arch

Stability for normal users, a release cadence we can ship snapshots of, and
apt as the update vehicle. Omarchy is the Arch answer; we are not Omarchy.

## What survives, what gets added

Already verified and stays: xdg-shell + layer-shell, SSD traffic lights +
snap flyout hook, workspaces 1–9, snap halves/quarters + assist picker +
snap groups, master+stack tiling, minimize/show-desktop, motion system,
accent presets + dark/light, cosmos-ipc socket surface.

Next gaps to close (each with guest evidence): XWayland (lazy start),
xdg-desktop-portal backend (screenshot + screencast + file chooser),
multi-monitor hotplug, fractional scaling 125%/150%, virtio-gpu real modes.

Disk layout is GPT + btrfs (`@`, `@home`, `@snapshots`, `@var_log`) + snapper
— the agent-rollback story depends on snapshots existing before cosmos-agentd.

# CosmosOS — Architecture

Strategy: **borrow the plumbing, own the experience.** Proven open-source
parts for the invisible layers; everything the user sees and touches is ours.

## Layers

```
┌─────────────────────────────────────────────────────────────┐
│ cosmos-shell (one process)                                  │
│ menubar · dynamic island · dock · Search-or-Ask · Start     │
│ panel+widgets · Control Centre · notifications · lock       │
│  — layer-shell surfaces, our design system (iced/cosmos-ui) │
├─────────────────────────────────────────────────────────────┤
│ cosmos-compositor — FORK of pop-os/cosmic-comp (GPL-3.0)    │
│ window engine: floating + tiling + workspaces + XWayland +  │
│ portals; our SSD chrome, snap flyout, motion, accent themes │
├─────────────────────────────────────────────────────────────┤
│ cosmos-agentd — agent runtime + MCP tools (desktop, a11y,   │
│ files, system, notify, clipboard, island)                   │
├─────────────────────────────────────────────────────────────┤
│ Debian 13 trixie — btrfs+snapper, zram, systemd-oomd,       │
│ PipeWire/WirePlumber, NetworkManager, BlueZ, flatpak,       │
│ xdg-desktop-portal, Calamares installer                     │
└─────────────────────────────────────────────────────────────┘
```

## Why cosmic-comp over Hyprland / our from-scratch compositor

- Same stack: Rust + Smithay, so our compositor knowledge and code port
  directly instead of being thrown away.
- Already has floating windows, auto-tiling, workspaces, multi-monitor,
  fractional scaling, XWayland, screencopy + xdg-desktop-portal plumbing —
  years of correctness we don't have to rebuild (protocol coverage alone is
  the argument).
- Hyprland is C++/wlroots with a different config/debug universe and no Rust
  surface; Omarchy already owns Arch+Hyprland as a distribution answer.

## Why Debian over Arch

Stability for normal users, a release cadence we can ship snapshots of, and
apt as the update vehicle. Omarchy is the Arch answer; we are not Omarchy.

## What carries over from the current compositor (port list)

The shell and all apps are Wayland *clients* — they survive the swap
unchanged (layer-shell is a protocol, not our code). What must be ported into
the fork:

- IPC surface (cosmos-ipc requests/events: launcher, help, assist, config,
  accent, tiling toggles) — retarget onto the fork's internals or keep the
  socket shape and rewire handlers.
- SSD chrome: traffic-light discs + centred title + rounded top corners;
  snap flyout hook on the zoom button.
- Window mgmt: snap halves/quarters + preview + assist picker, snap groups,
  minimize/show-desktop, workspace strip gestures, Super+keybind set.
- Motion system: open/close scale+fade, workspace slide, overlay slide-in.
- Damage/perf work: decal texture cache, release-gated shm handling
  expectations, software-GL path care (llvmpipe is a first-class tier).
- Accent preset + dark/light plumbing (cosmos-ipc config events).

Licensing: fork stays GPL-3.0, upstream copyright kept, no
System76/COSMIC/Pop!_OS user-visible branding. Upstream kept as a git remote;
fixes cherry-picked deliberately.

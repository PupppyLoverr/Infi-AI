# Decisions (ADRs)

Short records of the stack decisions in PLAN.md §2, with the evidence behind
them. New decisions get a new numbered section here.

## ADR-0001: Base OS is Fedora Atomic (bootc)

The OS is a container image built from `base/Containerfile`, converted to
bootable media by bootc-image-builder. Atomic updates and rollback are system
properties — required by the agent-undo goal (an agent must never leave the
machine in an unrecoverable state).

- Rejected: Debian trixie (the v1–v6 base) — frozen kernel/Mesa is wrong for a
  gaming-capable desktop; Arch — no atomic model, we would build rollback
  ourselves; NixOS — a second paradigm for contributors and general users.
- Fallback: if the 4 GB installed-size budget proves impossible on Fedora,
  Debian is re-evaluated with the budget numbers in hand (PLAN.md §7).

## ADR-0002: Window management is a niri fork (`solyra-comp`)

niri (Rust, GPL-3.0; upstream niri-wm/niri, 27k stars, v26.04 and active daily) is the
best-engineered compositor to start from: scrollable
tiling, quality animations, complete wlr protocol coverage, VRR, small and
fast. We fork it and add layout modes (master/stack, floating, snap zones +
Snap Assist), the Cosmos motion system, and an agent-surface protocol. The
GPL-3.0 applies to `solyra-comp` — acceptable for a distro.

- Rejected: writing another compositor on Smithay from scratch — the v1–v6
  core mistake (~16k LOC for a 7/10 result); Hyprland — C++, breaks the
  single-language Rust stack.
- Ship-order escape hatch: scrollable-tiling-only can ship first; the added
  layout modes are additive modules.

## ADR-0003: All shell + first-party apps are zgx

zgx (zeronsh) is a retained-mode Rust GUI framework: wgpu rendering (Vulkan,
GL fallback on driverless systems), fine-grained signal updates, idle windows
doing zero work, AccessKit accessibility, terminals and webviews built in.
Its published benchmarks show roughly half of GPUI's CPU and lower idle
memory. It is also our framework — the distro dogfoods and hardens it.

- zui (our GPUI fork) is available where immediate mode genuinely wins.
- At zgx `1b41efa`, `zgx-desktop` creates winit windows and implements its
  Wayland popups against their existing `xdg_surface`. Neither that host
  nor its vendored winit implements the layer-shell surface role. Our niri
  snapshot already supports `zwlr_layer_shell_v1`.
- First shell experiment: give one small panel its own Wayland layer-shell
  surface before attaching a buffer, then reuse zgx for its layout and
  rendering. Verify exclusive space reservation, pointer input, and panel
  removal restoring the work area in niri before choosing the final host.
  A compositor-pinned toplevel remains a fallback, not a layer-shell test.
- Risk cover: upstream originals (terminal, file manager, …) stay installed
  with a Default Apps switcher until zgx versions are stable — owner
  requirement, doubles as churn insurance while zgx is at 0.1.

## ADR-0004: zeron is the agent platform

`agentd` from v1–v6 is not rebuilt; its design (policy file, approvals in the
island, JSONL audit trail, MCP tool surface) carries over into the zeron
integration. Agent Space = dedicated workspaces now, per-agent micro-VM with
virtio-gpu + PipeWire display streaming later. Rollback = bootc (OS) plus
per-agent snapshots (files / VM disk).

## ADR-0005: The browser is chrome-first

Browser chrome (tabs, address bar, history, downloads) is zgx over
WebKitGTK — ships first, working day one. Servo embedding is a separate
research track with no committed date, reviewed quarterly.

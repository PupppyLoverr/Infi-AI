# Solyra OS — Rebuild Plan (v7: "the best Linux, for humans and agents")

This plan replaces the CosmosOS v1–v6 approach. The old code (own Smithay compositor,
own egui toolkit, CPU-rasterized shell) is archived, not deleted. What we keep from it
is its design language, its agent layer, and its verification culture. Everything else
starts from proven foundations.

---

## 1. What we are building

A real Linux distro, small and beautiful, where AI agents and humans share one desktop:

- **Agents are first-class desktop citizens.** Every agent gets its own desktop space
  (a dedicated workspace, or its own sandboxed VM with its own compositor) and can use
  it like a person would: open windows, read files, click, type. Humans watch, approve,
  and take over at any time. Nothing an agent does is unrecoverable.
- **The whole user-facing stack is one Rust UI framework: zgx** (fine-grained, retained,
  wgpu; idle windows do zero work). GPUI (via our `zui` fork) is used only where
  immediate mode genuinely wins.
- **Small and fast:** ≤ 4 GB installed, ≤ 3.5 GB ISO target, idle RAM ≤ 400 MiB
  (stretch: 300 MiB lean profile), boot-to-desktop ≤ 5 s on NVMe/KVM.
- **Runs on anything with UEFI:** x86_64 and aarch64 images, works on real GPUs and
  on VMs (GL fallback built in).
- **Best-in-class window management:** niri's scrollable tiling as the signature mode,
  plus master/stack, floating, and snap layouts, all with the Cosmos motion system.
- **For everyone:** general public (opinionated defaults), tiling fans (deep keyboard
  control), gamers (fresh Mesa, VRR, HDR roadmap, gamescope), themers (one theme
  format that reaches every first-party app), and agents (the agent platform).

## 2. Stack decisions (and what we rejected)

### Base OS: Fedora Atomic (bootc) — trimmed

The whole OS is an OCI container image built by a `Containerfile`, converted to a
bootable image/ISO by bootc-image-builder. Atomic updates, one-click rollback, and
CI-built images are native properties, not projects we write.

- **Why not Debian (as v1–v6 did):** gamers need current kernel + Mesa; Debian trixie
  freezes both. Fedora ships Mesa days after upstream.
- **Why not Arch:** no atomic model; we'd rebuild rollback ourselves. (Omarchy works
  because DHH accepts that tradeoff. We don't.)
- **Why not NixOS:** great reproducibility, but a second, steep paradigm for
  contributors and for "general public".
- Sizing: ublue-style minimal base ≈ 1.6–2.2 GB; adding our shell/apps/agent stack and
  trimming (no GNOME, no Toolbox extras) lands at **~3 GB installed**. Budget reviewed
  every milestone (see §7).

### Window management: fork niri → `solyra-comp`

niri (Rust, GPL-3.0) is the best-engineered compositor we can start from: scrollable
tiling, excellent animations, wlr-protocols complete, VRR support, tiny and fast.

Package the vendored niri build as `solyra-comp` and add:
1. Add **layout modes** per workspace: scrollable (default), master/stack, floating,
   and Windows-11-style snap zones with Snap Assist (the v1–v6 model, which was good).
2. Port the Cosmos motion curves and glass/material pipeline (GPU-side now, not the
   pixman path v1–v6 used).
3. Add a small **agent surface protocol**: agents claim a workspace or an output,
   and the compositor tags their windows for the shell and audit layer.

Rejected: writing another compositor from scratch (that was v1–v6's core mistake —
~16k lines of Smithay code for a 7/10 result), and Hyprland (C++; we'd lose the
single-language Rust stack and its plugin/config model doesn't match ours).

### Shell + apps: zgx everywhere

zgx already runs natively on Wayland (winit), draws through Vulkan with GL fallback,
has terminals, webviews, a component library, IME, drag-and-drop, and AccessKit
accessibility. Its own benchmarks show roughly half of GPUI's CPU and lower idle
memory. It is also *our* framework — building the distro on it dogfoods and hardens it.

- **Shell** (`solyra-shell` in zgx): menubar, dock (bottom/left/right), launcher
  (Search-or-Ask), Control Centre, notifications, dynamic island, window switcher,
  cheatsheet — the v1–v6 feature set, re-skinned on the new engine.
  - **Spike 1 (first code we write):** layer-shell surfaces in zgx (wlr-layer-shell-v1
    via a small winit/vendor patch or a dedicated zgx backend). Without it, the shell
    runs as pinned toplevels the compositor treats specially — acceptable fallback,
    worse result. This spike decides the shell architecture; do it in week 1.
- **Core apps, rebuilt in zgx, gradually:** terminal, files, editor, settings, monitor.
- **Originals stay, by design:** upstream apps (e.g. a mature terminal and file
  manager) ship in the image until our zgx versions are stable. Settings gets a
  **Default Apps** switcher; the launcher shows both until a zgx app is marked default.
  Nothing is removed on day one.

### Browser: chrome in zgx, engine pragmatic

"Browser in Rust" means two tracks, honestly separated:
- **Track A (ships first):** browser chrome — tabs, address bar, history, downloads —
  in zgx around WebKitGTK (zgx's webview today). Working day one.
- **Track B (research track):** Servo embedding as the engine behind the same chrome,
  gated on Servo's embedding API maturity. No promises, no deadline, reviewed quarterly.
- Port the useful host/integration pieces from `zeron` (its desktop app's session,
  sync, and webview plumbing) rather than rewriting them.

### Agents: Solyra uses zeron as its agent platform

`zeron` (control plane for Claude Code, Codex, Cursor, Devin…) becomes the distro's
agent layer instead of rebuilding `agentd` from scratch:

1. **Agent Space** — a dedicated workspace (or set of them) where agent-owned windows
   live, visually distinct, excluded from the human's alt-tab unless visited.
2. **Sandbox levels:** level 1 = separate user + bubblewrap + policy file (the v1–v6
   `cosmos-agent.toml` design, kept); level 2 = per-agent micro-VM (crosvm/QEMU,
   virtio-gpu) whose display streams into Agent Space over PipeWire and whose input
   the agent drives via a virtual seat. Level 2 is the "own sandbox VM" goal.
3. **Approvals + audit** in the island/notification UI (v1–v6 design kept verbatim —
   it was right): every sensitive tool call asks in the UI; every call is logged to a
   per-agent JSONL audit trail.
4. **Undo:** bootc rollback for OS-level changes; per-agent btrfs/LVM snapshots (or
   VM disk snapshots at level 2) for file changes. The Phase-6 v1–v6 demo — agent
   renames a file, asks to delete another, is approved, then rolls the task back —
   is the demo we will beat.
5. **opencode and friends** preinstalled; zeron desktop app ported to zgx over time.

## 3. Repository layout (after reset)

```
README.md            — the project, in plain words
PLAN.md              — this file (living)
docs/
  design-language.md — tokens, type, metrics, motion, surface inventory
  apps.md            — first-party app plan and the Default Apps switcher
  setup.md           — first-boot wizard flow, including the gaming bundle
  agent-platform.md  — agent space, sandboxing, approvals, audit, zeron integration
  budgets.md         — RAM/storage/boot budgets, measured every milestone
  decisions.md       — ADRs: every stack decision above, with evidence
base/                — Containerfile, image config, ISO/build pipeline, CI
comp/                — solyra-comp (niri fork) + our patches
shell/               — solyra-shell: the panel layer surface first, zgx host later
apps/                — terminal, files, editor, settings, monitor (zgx), one crate each
agent/               — agent-space bridge, policy engine, VM runner, MCP tools
assets/wallpapers/   — original wallpaper art (kept from v5/v6)
tests/drive/         — QEMU QMP drive harness (kept; engine-agnostic)
```

The previous implementation (CosmosOS v1–v6, `cosmosos/` tree) is preserved on the
`cosmos-legacy` and `cosmos-legacy-v6` branches, not deleted.

## 4. Phases (each ends with something bootable and verified)

- **P0 — Reset (day 1).** Push current tree to `cosmos-legacy` branch. Wipe main to
  this plan + curated docs. Stand up CI (image build on every main push).
- **P1 — Base image.** Containerfile → bootc image → raw disk → boots to a TTY.
  Measured: install size, idle RAM of bare boot, boot time. Gate: boots on QEMU x86_64
  + aarch64. The x86_64 prototype reached multi-user in QEMU and used 2.0 GiB
  for its root filesystem. Serial login, idle RAM, and aarch64 remain unverified.
  The local raw-disk path uses a development GRUB configuration; installer media
  and production update/rollback testing are separate milestones.
- **P2 — Window management.** niri builds in our image and runs from a
  cold-booted disk in QEMU: the session owns a DRM output, opens Foot and
  Thunar on Super+Enter and Super+E, tiles them into two columns, moves focus,
  and opens the workspace overview. The session imports its environment, and
  X11 apps get a display through xwayland-satellite. Gates still open: snap
  zones with Snap Assist (compositor work, not yet written), the per-feature
  `tests/drive` scripts, and a hardware run.
- **P3 — Shell.** The layer-shell spike is done: `shell/panel` is a 32 px bar on
  `zwlr_layer_shell_v1` that reserves space (verified in a nested session), draws
  the palette, and reads workspace state from niri's IPC. Next: the status
  cluster and pointer input on the bar, then the dock, launcher, and control
  centre. Gate: idle RAM budget met with the shell running; 60 fps animations
  on llvmpipe and on real GPU.
- **P4 — Apps v1 + originals.** Spec in `docs/apps.md`: terminal first, then files,
  then settings with the Default Apps switcher; upstream apps stay installed and
  selectable. Gate: the daily-driver checklist in that document passes.
- **P5 — Agent platform.** Spec in `docs/agent-platform.md`: Agent Space on reserved
  workspaces, level-1 sandbox (dedicated user, bubblewrap, policy file), approvals,
  JSONL audit, and a rollback story. File snapshots need a root filesystem decision
  because the development disk is XFS. Gate: the rollback demo passes end-to-end.
- **P6 — Browser shell** (Track A), per-agent VMs (sandbox level 2), the first-boot
  wizard in `docs/setup.md` (including the Flatpak gaming bundle), theming system,
  installer (Calamares against the bootc image), aarch64 polish.
- **P7 — 1.0.** Soak tests, budgets audit, docs, first public ISO.

## 5. What we keep from the old tree

- `docs/design` knowledge: design language spec, accent presets, wallpapers (original
  assets — keep; they're ours and good), reference screenshots.
- The **agent policy + approval + audit design** (docs/agentd.md) — carried into
  `docs/agent-platform.md`.
- The **QEMU drive harness** (`tests/drive`) — the only part of the test stack that is
  engine-agnostic; it verified real boots, and we'll reuse it from day one.
- Budgets discipline (`docs/budgets.md`): every milestone reports measured numbers.

## 6. Honest risks

| Risk | Reality | Mitigation |
| --- | --- | --- |
| zgx is 0.1, API churns | Real; we'd build the whole desktop on it | Pin zgx revisions; contribute fixes upstream; originals stay installed (the user-required escape hatch doubles as risk cover) |
| No layer-shell in zgx today | Confirmed from zgx docs | Spike 1 decides shell architecture in week 1; fallback is pinned toplevels |
| niri fork needs layout modes added | Real work in a foreign codebase | Ship scrollable-only first if needed; layout modes are additive modules |
| 3–4 GB budget vs bootc tooling | Base images carry some weight | Aggressive package trimming; measured every milestone; Debian fallback if Fedora can't hit it |
| Servo embedding | Not ready on anyone's timeline | Track B has no ship date; Track A ships |
| Doing everything at once | The classic failure | Phases ship bootable increments; no phase starts before the previous gate passes |

## 7. Budgets (hard, measured)

| Metric | Target | Stretch |
| --- | --- | --- |
| Installed size | ≤ 4 GB | ≤ 3 GB |
| ISO | ≤ 3.5 GB | ≤ 2.5 GB |
| Idle RAM (compositor+shell+services) | ≤ 400 MiB | ≤ 300 MiB |
| Compositor RSS | ≤ 120 MB | ≤ 80 MB |
| Shell RSS | ≤ 90 MB | ≤ 60 MB |
| Boot (UEFI → first frame) | ≤ 5 s | ≤ 3 s |
| Agent (level 1) overhead | ≤ 60 MiB | ≤ 30 MiB |

## 8. Decisions (owner's answers, 2026-11-18)

1. **Name:** owner's picks and every checked candidate — Vela, Lyra, Zenith, Nimbus,
   Vesper, Nexiloop, Solara, Velora, Solvena, Noctra, Zunari, Vantora, Solyx, Helvora —
   are in use somewhere. **Naming rule adopted:** fatal = an OS/distro or tech giant
   owns it; risky = an AI/agent company owns it (too close to our story); acceptable =
   a distant industry (everyone has those). Owner chose "something similar to Solara";
   **Solyra adopted as the project name.** Category check: no OS, distro, desktop or
   tech giant uses it (clear), but several small software/AI studios do (risky tier) —
   final branding-time review before any public release; rename is cheap pre-branding.
   A lowercase working name is used for P0–P2.
2. **Gaming defaults:** included in the **first-boot setup wizard** (Windows OOBE /
   macOS Setup Assistant style) — the wizard offers Gaming alongside network, accounts,
   appearance, and other setup choices as one-click optional layers. Keeps the base
   image small; gaming is a choice at setup, not baked in. The setup wizard itself is
   now a planned zgx app (added to P6 scope).
3. **Fork bases considered:** Pop!_OS (Ubuntu + COSMIC/iced — conflicts with the zgx
   stack; take its installer/driver lessons only) and Zorin (customized GNOME — take
   design lessons only, not code). Decision: keep Fedora Atomic + niri fork.
4. **Base OS** stays Fedora Atomic unless the 4 GB budget proves impossible.

# CosmosOS — Status

Current state of the tree. Updated at each phase boundary.

## What exists

**Base OS** — Debian 13 (trixie) rootfs assembled by `image/build.sh` (mmdebstrap →
msdos/ext4/GRUB raw image), `image/run.sh` boots it in QEMU. Guest boots to an
autologin session running our own Wayland stack. opencode, foot, firefox-esr and
the curated CLI set (ripgrep/fd/fzf/eza/bat/btop/fastfetch/neovim/tmux/lazygit)
are packaged into the image; NetworkManager + polkit + netdev group are wired
for the tray toggle.

**cosmos-compositor** (`cosmos/compositor`, ~13.5k LOC Rust on Smithay 0.7) —
udev (DRM/GBM, llvmpipe-capable) + winit backends, xdg-shell + layer-shell,
SSD chrome (macOS traffic lights, centered title), workspaces 1-9, snap L/R +
maximize + drag-edge preview + Snap Assist picker, master+stack tiling
(Super+T), minimize/show-desktop, motion system (fade/scale/slide), accent
theme presets, drop shadows, vignette wallpaper, IPC socket (cosmos-ipc),
QMP drive harness instrumentation.

**cosmos-shell** (`cosmos/shell`) — layer-shell surfaces in one process:
menubar (logo + focused app + workspaces + clock + tray icons), LEFT-edge
vertical dock (pinned+running apps, magnification, hover labels, separators —
exclusive zone reserved so windows never overlap it), Spotlight-style
launcher (search + pinned grid + recommended), Quick Settings flyout
(volume slider via wpctl, network toggle via NM D-Bus, accent dial, dark/light
pill), notification daemon, Snap Assist card, zoom flyout (Win11 snap-layouts
card on green-button dwell → snap into thirds/quarters/wides/max), super+?
cheatsheet, window switcher. Inter + JetBrains Mono throughout.
Dynamic island (PR #101): centred menubar pill (clipboard snippet +
staged-file badge) expanding into a `cosmos-island` card — CLIPBOARD
history ring of 10 fed by a wlr-data-control watcher + STAGED FILES
via text/uri-list drops; uitk apps publish/consume a real clipboard
(copy-paste source + egui Paste events). Launcher is Search-or-Ask
(PR #102): unified rows — apps, file index hits (bg thread over
~/Documents etc, 20k cap), live calculator, `>` run commands, `?` Ask
(opencode) — kind icons + subtitles + Enter/click activate.
Start widgets (PR #103): the idle launcher view is a true Start panel —
PINNED grid + RECOMMENDED + WIDGETS (Clock / System / Storage cards with
live /proc + statvfs data) + footer; result rows only while searching.
Control Centre (PR #104): Quick Settings becomes grouped macOS modules —
connectivity+sound and appearance+focus cards — plus a real Focus
(Do-Not-Disturb) toggle that suppresses notification popups and shows a
moon in the menubar tray.

**cosmos-portal** (`cosmos/portal`) — xdg-desktop-portal backend
`org.freedesktop.impl.portal.desktop.cosmos`: Screenshot (compositor IPC →
GL offscreen PNG), PickColor (centre pixel), FileChooser (cosmos-files
`--chooser` picker, real file paths), ScreenCast (real PipeWire producer
node streaming BGRx frames captured via IPC screenshots). `.portal` +
`.service` data files ship in `portal/data/`; image wiring in flight.

**cosmos-uitk** (`cosmos/uitk`) — egui toolkit: sctk plumbing + software
rasterizer (per-pixel barycentric), release-gated shm buffers, per-cell ANSI
terminal rendering lives in apps/terminal.

**Apps** (`cosmos/apps`) — terminal (vt100 + pty + 256/truecolor + mouse +
bracketed paste + `-e`), files, editor, settings, monitor.

**Verified on guest (drive harness, QEMU screendumps)**: colours, shadows,
launcher/flyout/dock, snap+assist, focus chrome, notifications, network toggle,
opencode/htop/nvim rendering, zero-panic soaks. Budgets @17244cf: image 1.6G
real / 3.0G sparse, idle RAM ~330–410MiB, boot→panel 5.4s.

**Compositor gaps closed since the audit**: lazy XWayland via
`xwayland_shell` + satellite-owned XWM (PR #91), fractional output scale
125/150% honored (PR #92), IPC screenshot capture (PR #93), portal backend
Screenshot/PickColor/FileChooser/ScreenCast (PRs #94–#96), GPT+btrfs
subvolumes+snapper disk layout (PR #97, budgets 1.8G real / ~242MiB idle /
6.68s boot). Snap zones extended to 13 states with one shared geometry
table — the zoom flyout's layout thumbnails and the compositor's snap
rects can't drift (shared `cosmos_ipc::SNAP_LAYOUTS`). Remaining:
multi-monitor hotplug guest proof, virtio-gpu real modes, portal image
wiring — all in flight on the image host.

## Papercuts — fixed and boot-verified (PR #88)
Hostname `cosmosos` (DMI product name `CosmosOS` + PS1 fix), en_US.UTF-8
generated + defaulted via update-locale, zram lz4 @ 50% RAM, QEMU-follow
resolution via `run.sh GUEST_RES=WxH` (virtio-vga xres/yres, verified
1024x768 + 1280x720). Evidence: `docs/evidence/01-papercuts.png`.

## Direction (post Phase-1 audit)
Fork of pop-os/cosmic-comp CANCELLED — our compositor stays; cosmic-comp is
a read-only lift catalog (see `docs/LIFT.md`). Shell stays egui on uitk;
GPU render path (egui-wgpu) added by tier with the software rasterizer as
the Lite fallback. Disk layout is GPT + btrfs subvolumes + snapper (done,
PR #97). Dock is left-edge vertical (done, PR #90). Feature ladder: zoom
flyout (done, PR #99) → dynamic island (done, PR #101 — guest
verification in flight) → Search-or-Ask (done, PR #102) → Start
panel+widgets (done, PR #103) → Control Centre (done, PR #104) →
greetd+lock (done
code-side: ext-session-lock in compositor + `cosmos-lock` PAM client +
`cosmos-greeter` greetd greeter, PRs #106–#107; image wiring on the
image host) → cosmos-agentd →
ISO+Calamares.

## Session lock + greeter — code complete (PRs #106–#107)
- Compositor speaks `ext-session-lock-v1`: `super+L` spawns `cosmos-lock`;
  while `cosmos.session_locked` the render path composites only wallpaper
  + lock surfaces and all input routes to them; `unlock()` restores
  pointer focus. Lock client death auto-unlocks (fail-safe).
- `cosmos-lock`: sctk session-lock client, egui card (clock, avatar,
  autofocused password) on every output, PAM auth via the `cosmos-lock`
  service (pam 0.8 `Client::with_password` → `set_credentials` →
  `authenticate`). Release-gated `attach_to` shm like uitk.
- `cosmos-greeter`: greetd greeter under cage — `greetd_ipc` sync-codec
  (`CreateSession` → `PostAuthMessageResponse` → `StartSession` with
  `cosmos-session`), worker-thread auth so the card stays live.
- Control Centre footer gained a Lock button (Settings|Lock|Log out).
- Still needs (image host): binary installs, `/etc/pam.d/cosmos-lock`
  (common-auth), `greetd` + `cage` packages, `/etc/greetd/config.toml`,
  `/usr/share/wayland-sessions/cosmos.desktop`, greeter/lock drive-verified
  screendumps.
- **Guest-verified (PR #111 + lock drive):** greetd→cage→greeter card →
  login → cosmos session, zero panics; `03-greeter-card.png` evidence.
  agentd MCP smoke 10/10: initialize → serverInfo, `tools/list` → 15
  tools, `desktop.windows.list` real table, `files.write` deny+allow,
  audit JSONL — transcript `docs/evidence/agentd-mcp.txt`.
- Defects found by the drive and fixed (PR #112): shell abort on any
  external clipboard offer (missing `event_created_child` in the zwlr
  data-control dispatch — killed the whole panel on `wl-copy`); greeter
  Enter not submitting when egui keeps focus. Re-verify in flight.
- Open: whether the ~42s `greeter exited without creating a session`
  respawn was a legit session-end (greetd restarts the greeter by
  design) or a real defect — child is checking serial.log markers.

## cosmos-agents — the Agents app (PR #114)
- `cosmos-agents` (uitk): agent list from `~/.config/cosmos/agents/*.toml`
  + audit-only agents (flagged "default policy"), parsed policy detail,
  live audit feed per agent (latest 200, errors red, 3s refresh), and a
  snapper rollback pane via `pkexec snapper -c root rollback <n>`.
- Image wiring needed: add `cosmos-agents` to the build loop + install
  line in `image/build.sh`; a polkit rule for `pkexec snapper` as the
  cosmos user; `.desktop` lands via the existing glob.

## cosmos-agentd — approvals + notification actions (PR #113)
- The freedesktop daemon now parses `actions` (key/label pairs),
  advertises the `actions` capability, draws button pills on the card,
  and emits `ActionInvoked`/`NotificationClosed` back to the client.
  `urgency=critical` bypasses Focus; negative timeouts are persistent.
- agentd `sensitive` tools no longer fail closed: the call posts a
  Deny / Allow once / Always allow card and blocks up to 120s for the
  user's decision (Allow-once per call; Always for the agentd's
  lifetime; dismiss/timeout → a clear error to the agent).

## cosmos-agentd — slice A merged (PR #108)
- `cosmos-agentd` unix-socket MCP server (JSON-RPC `initialize`/
  `tools/list`/`tools/call`): 15 real tools — `desktop.*` over the
  compositor IPC, `files.*` scoped by `~/.config/cosmos/agents/
  <name>.toml` policy (read_roots/write_roots/tools/sensitive),
  `system.settings.*`, JSONL audit log (§6.8). `sensitive` tools fail
  closed until the approval slice lands.
- `docs/agentd.md` has the full §6 design + slice order (identities/
  sandbox → approvals+island → headless seat → rollback → onboarding →
  skills).
- Image host: binary + systemd user unit + MCP smoke transcript in
  flight.
- Slice A.1 (PR #109): `--stdio` transport — the same handler on
  stdin/stdout so MCP-capable agents (opencode/Claude Code) attach with
  one config line.
- Slice G started (PR #110): `cosmos/skills/` (→
  `/usr/share/cosmos/skills/`, §6.11) + custom widgets real —
  `~/.local/share/cosmos/widgets/<name>/` (widget.toml + data.sh)
  renders in the Start WIDGETS strip, cached per refresh, 2s kill cap.

## What's broken / open
- Snap Assist tray-click is fixed in code (bbox pass-through #50, clipped
  input region + dismiss-on-outside-press #51, free-half respects the left
  rail #90); regression-covered permanently by
  `tests/drive/assist-tray.sh` (IPC-asserted every round).
- llvmpipe-only known limits: no live blur until the GPU shell path lands.

## Build
`cosmosos/image/build.sh` on an x86_64 Linux host (provision via
`provision/host-deps.sh`) → `dist/cosmos.raw`; `run.sh` for KVM/TCG boot
(`GUEST_RES` for resolution, OVMF for the GPT/UEFI image).
Rust workspace compiles via `cargo check --workspace` in `cosmos/`.

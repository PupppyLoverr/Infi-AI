# Solyra plan: macOS 27 "Golden Gate"-style distro

Source of truth: `docs/BRIEF.md` (the user's brief, verbatim). This file maps
every requirement in it to work. Earlier plans: `docs/plan-2026-niri-fedora.md`.
Status: **decided 2026-11-26 (§8)**: Hyprland + Quickshell on Arch,
≤250 MB idle budget. M1b (measure that stack) is next.

## 1. The problem (Milestone 1 result)
Plasma 6 gives pearOS-level fidelity but measured **1144–1192 MB idle,
3.0 GB disk** (CI run 45813098984: Arch, 2 vCPU / 4 GiB, llvmpipe,
5 min after login). That is about 7× the 150 MB target. The trim (file
indexing off) saved 48 MB; stock `plasma-desktop` installs no extras to
remove. Largest processes: plasmashell 517 MiB, kwin 305, kscreenlocker 272
(auto-lock fired), kded6 63, powerdevil 56, portal 54, ksecretd 51,
kaccess 51, polkit 50, activitymanager 41, Xwayland 39.

Our current stack (niri fork + CPU-drawn Rust panel) is light, but per the
user it cannot reach identical Golden Gate UI/UX: no GPU scene graph, so
vector shapes, springs, blur-aware layering and fine typography all have to
be done by hand.

## 2. Proposal: option (c), Plasma's toolkit without Plasma
Keep what makes Plasma look good (Qt Quick / QML, GPU scene graph,
shaders, the same language as pearOS's components). Drop what makes it heavy
(plasmashell, kded, activities, KWin's scripting stack, kscreenlocker).

| Layer | Choice | Why |
|---|---|---|
| Base | Arch Linux (as the brief says; pearOS's base) | Brief; pearOS packaging reuse |
| Compositor | **Hyprland** (BSD-3), version pinned (recommendation, see §3) | Floating windows like macOS, bezier animations, rounding, shadows, blur, plugin API, IPC |
| Glass | Compositor plugin (GPL-3.0, its own component): our refraction shader from `comp/` + liquid-gel's algorithm (rewritten for our pipeline if too heavy) | Real backdrop blur and lensing behind every surface; one transparency slider |
| Shell UI | **Quickshell** (Qt Quick/QML shell toolkit for wlroots and Hyprland, LGPL-3.0): dock, menu bar, global menu, notch, Spotlight, Control Center, notifications, widgets, launcher, lock (ext-session-lock), OSDs | Same tech as pearOS (QML), so pearOS QML logic and measurements port with less change. GPU-animated. ~1/5 of Plasma's RAM (to be measured) |
| Services | Rust daemons over D-Bus: media (MPRIS), clipboard, file search index, AI "Ask" provider, window arrangement engine | Rust where it pays: fast and small, no UI |
| Window decorations | Client-side for our apps; compositor borders and shadows; traffic-light titlebar via a plugin (or Quickshell overlay) | Uniform corner radius and toolbars |
| Terminal | zgx + zeron engine (brief B), unchanged | MIT; separate from GPL glass |
| Apps | Existing zgx apps (Files, Settings, Browse, Setup) restyled | Don't rebuild what works |

**What we keep from today:** the zgx apps, the glass shader math and
`solyra_glass_v1` ideas, the Rust media, clipboard, shelf and lyrics logic
(moving into services), the design tokens, wallpapers and Solar icons.

**What gets retired:**
- the niri fork (its glass gets ported);
- the CPU-canvas panel (its logic becomes services and QML);
- `web/` (Tauri). Its terminal design stays as the zgx terminal's reference;
- the Fedora bootc base.

**Estimate to verify in M1b** (GPU; llvmpipe inflates):
- Hyprland: 40–80 MB.
- Quickshell with all surfaces: 70–130 MB.
- System (systemd, journald, udev, dbus, PipeWire + WirePlumber,
  NetworkManager): 60–90 MB.
- **Total: about 180–300 MB idle, 2–2.5 GB installed.**

That does not reach 100–150 MB. No GPU-composited QML desktop does. See
question 2.

## 3. Compositor: niri vs Hyprland vs others
| | niri (today) | **Hyprland** | Wayfire | KWin alone |
|---|---|---|---|---|
| Window model | Scrolling tiling; floating is secondary | Floating or tiling per workspace | Floating-first | Floating |
| Win 11-style snap layouts | Doesn't fit the model | Shell overlay + IPC moves (works on floating) | Plugin | Built in (quick tile) |
| "Auto-arrange on open" | Always tiles | Toggle a workspace to dwindle/master tiling | Plugin | Script |
| Mission Control / overview | Overview exists | hyprexpo plugin or our own | Expo/scale plugins | Overview effect |
| Animations | Good | Bezier curves for every event | Good | Best |
| Blur / glass | Ours (refraction) | Built-in blur; plugin for lensing | Blur plugin | Effects (liquid-gel) |
| RAM | Low | Low to medium | Low | ~300 MB measured |
| Risk | Our fork to maintain | Plugin ABI changes each release: pin it | Smaller community | Weight |

**Recommendation: Hyprland.**
- macOS is a floating-window desktop, and the user's Windows 11 snapping and
  auto-arrange both map directly onto Hyprland's floating and tiling
  workspaces.
- Quickshell has first-class Hyprland integration.
- Cost: porting our glass shader into a Hyprland plugin (one milestone).

## 4. Requirement map (every line of the brief)
**Role and mode:**
- Work autonomously; ask only before destructive, irreversible, costly or
  licence-breaking steps.
- Continue this repo; reuse what works (§2 keep list).

**Goal:**
- Golden Gate layout, behaviour, motion and glass.
- Only original or openly licensed icons (Solar CC BY 4.0, ours),
  wallpapers (ours), sounds (pearos-sounds GPL-3.0 or ours) and fonts
  (Inter, JetBrains Mono, OFL).
- Our own names. No Apple assets, logos or trademarks.

**A. Shell:**
- The brief asked for Plasma/KWin components. M1 showed Plasma misses the
  budget, so §2 keeps the toolkit (QML/Qt, C++ where needed) and drops
  Plasma. **The user decides.**
- No Tauri or React for shell components.
- pearOS repos: listed (26, all pages) with licences checked (table §7).
  Reuse where licensed. Check what exists before each component.
- Spotlight: KRunner's model (plugins and runners) as reference. Our search
  is a Rust index service plus a QML bar.
- pearOS notch: unproven. Built fresh, profiled for frame drops.

**B. Terminal:**
- zgx pinned to exact commits (today `699ae02e`).
- zeron engine (alacritty_terminal + portable-pty) ported. Its view is
  rewritten as a zgx component; no GPUI.
- Required features: tabs, splits, scrollback, selection, copy and paste,
  search, truecolor, Unicode, resize, themes, adjustable transparency, and
  the user's shell with shell integration. Search zeron's source before
  claiming any reuse; if absent, implement OSC 7 / OSC 133 ourselves.
- Blur spike first (M2) on the chosen compositor. Smallest platform patch
  if zgx can't make a transparent window; documented.

**Budgets:**
- RAM 100–150 MB, disk 2–3 GB, ISO under 3 GB.
- Fast boot, instant launch and menus, refresh-rate animations, near-zero
  idle CPU.
- Make an effect cheaper before removing it.
- Every milestone re-measures with `tools/measure`, adapted per stack, never
  excluding processes. A regression is a bug.

**Golden Gate behaviour:**
- One glass transparency slider (ultra clear to fully tinted, text readable
  at every step: contrast-tested).
- Diffusion of busy backdrops, active-window contrast, improved shadows.
- Uniform corner radius, toolbars and edge-to-edge sidebars; layered app
  icons.
- Super+Space "Search or Ask" with a pluggable AI provider.
- Background-running dock indicator plus "Stop Running in Background".
- Smooth workspace and overview animations.
- Show Borders, Reduce Transparency, Reduce Motion.
- Anything else gets checked against Apple or a reputable source first.

**User extras (M9):**
- Droppy parity: every getdroppy.app feature we can do (notch, lock screen,
  media and more) on real MPRIS media. The inventory comes from Droppy's own
  docs (getdroppy.app is blocked by this sandbox's proxy; the repo docs
  are the source).
- Windows 11-style management: snap layouts (hover the maximise button,
  zones, snap groups), multiple organised windows per screen, and an
  optional auto-arrange when windows open.

**Suggested extras** (not built unless approved):
1. Mission Control / App Exposé.
2. Stage Manager.
3. Hot corners.
4. Quick Look (Space in Files).
5. Clipboard history in Spotlight.
6. Night Shift.
7. Focus modes.
8. Screenshot and recording tool with markup.
9. Per-app volume in Control Center.
10. Desktop widgets.

**Licensing:**
- Keep copyright notices.
- GPL code (liquid-gel and our changes to it) stays in the GPL glass plugin,
  never in the MIT terminal.
- Maintain `THIRD_PARTY.md` (rename from THIRD_PARTY_NOTICES.md in M2).

**Usage rules:**
- No subagents.
- Small steps, each committed, with HANDOFF.md updated.
- Short output.
- No full ISO build before M8.
- Above 80% usage: finish the edit, update HANDOFF.md, stop.

**Definition of done (every milestone):**
- It builds, runs, and has been tested on the chosen desktop.
- RAM, disk and boot numbers recorded, with a screenshot.
- Untested, stubbed or broken parts stated plainly.

## 5. Master execution plan (one shot, autonomous)
Ground rules for every step:
- Per the brief: no subagents, short output, build only what changed.
- After each step: commit, update HANDOFF.md, push to the dev branch, then
  fast-forward main (check heads first).
- **Test bed:** this container cannot run Arch or Hyprland (no KVM, Arch
  mirrors blocked), so every on-desktop test runs in CI.
  - The `solyra-vm` workflow: Arch cloud image under QEMU/KVM, our packages
    installed, autologin into Hyprland, `grim` screenshots, `free -m`, disk
    and boot numbers, all uploaded as artifacts.
  - `tools/measure` grows into this.
- **Done** = it builds, runs in the VM on Hyprland, has numbers and a
  screenshot, and limits are stated plainly.

### M0. Infrastructure (prerequisite)
- [ ] `pkg/` holds PKGBUILDs for every Solyra component; CI builds them in an
  `archlinux` container into a local pacman repo artifact.
- [ ] `solyra-vm` workflow: install the repo into the VM, run the session,
  then a scenario script (e.g. `hyprctl dispatch exec`, `qs ipc call`) and
  `grim` shots at 1280×800; upload shots and `report.txt`.
- [ ] A budget gate: fail CI if idle RAM > 250 MB (llvmpipe noted) or
  installed size > 3 GB.

### M1. Audit and measure ✅ (Plasma 1144–1192 MB). M1b: Hyprland + Quickshell (running)
- [ ] Record the M1b numbers. If over 250 MB, trim (portals, Xwayland
  on demand, PipeWire socket activation) and re-measure before going on.

### M2. Terminal blur spike (zgx on Hyprland)
- [ ] `apps/solyra-term` minimal: a zgx window with a transparent clear
  colour, pinned zgx rev.
- [ ] Hyprland `windowrule` blur for app-id `solyra-term`. Screenshot over a
  busy wallpaper.
- [ ] If zgx can't make the surface transparent: the smallest patch
  (alpha mode, opaque-region removal), carried in `apps/patches/`, with
  the reason documented.

### M3. Terminal core
- [ ] Port zeron's engine (MIT, notices kept): `alacritty_terminal`
  Term/grid, `portable-pty` spawn of the user's `$SHELL`, read loop, resize.
- [ ] zgx terminal view: glyph atlas text, truecolor, Unicode width (wide
  and combining), cursor, keyboard and mouse input, scrollback, selection,
  copy and paste (wl-clipboard), search.
- [ ] Shell integration: search zeron's source first. Else implement OSC 7
  (cwd) and OSC 133 (prompt marks) with bash/zsh/fish snippets.
- [ ] Tests: emulator unit tests (2J/3J `clear`, wide chars, resize reflow),
  PTY echo test. VM screenshot running `htop` and a truecolor test.

### M4. Glass, theme tokens, transparency slider
- [ ] `solyra-glass`: a Hyprland plugin (GPL-3.0, own repo dir and
  licence).
  - Port our refraction shader (rim lensing, diffusion, adaptive tint,
    specular rim, shadow) from `comp/`.
  - Take liquid-gel's blur algorithm where it is lighter; rewrite it where
    it is heavy. Pin the Hyprland version.
- [ ] One `glass` value 0–100 (ultra clear to fully tinted) in
  `~/.config/solyra/look.toml`, read by the plugin (hyprctl keyword), by
  Quickshell (file watch) and by zgx apps. Text contrast ≥ 4.5:1 tested at
  0/50/100.
- [ ] Tokens: corner radius (one value everywhere), shadows (two-layer),
  active-window contrast, Show Borders, Reduce Transparency (forces
  tinted), Reduce Motion (animations off or crossfade).

### M5. Menu bar and dock (Quickshell)
- [ ] Menu bar: logo menu, app name, global menu (DBusMenu via the
  `org.kde.kappmenu` protocol and the appmenu registrar; GTK through
  `appmenu-gtk-module`), status items (SNI tray), Control Center button,
  clock.
- [ ] Dock:
  - pinned and running apps from `hyprctl clients`;
  - Gaussian magnify and bounce (pearos-dock GPL-2.0+ numbers);
  - Downloads stack and Trash;
  - background-running indicator (apps with no windows but a live process
    or tray item) plus "Stop Running in Background" (SIGTERM, then
    SIGKILL after 5 s);
  - layered icons (Solyra icon set, foreground and background layers).
- [ ] Rust `solyra-shelld` services over D-Bus: media (MPRIS), clipboard,
  shelf, file index, apps; ported from the panel.

### M6. Spotlight, notch, Control Center, notifications
- [ ] Spotlight (Super+Space), "Search or Ask":
  - instant local search over apps, files (Rust index, inotify), settings,
    calculator, clipboard and actions;
  - Ask tab with pluggable providers (claude CLI with the Claude
    subscription login, codex CLI with ChatGPT login), streamed answers.
- [ ] Notch: built fresh. Morph animations at refresh rate (profiled with
  `QSG_RENDER_TIMING`); compact live activities; media.
- [ ] Control Center: Wi-Fi (NetworkManager D-Bus), Bluetooth (BlueZ),
  sound and display sliders, Focus, Now Playing, the transparency slider.
- [ ] Notifications: an `org.freedesktop.Notifications` server in Quickshell,
  with banners, a stacked centre and actions.
- [ ] Lock screen (ext-session-lock + PAM), widgets, launcher grid, OSDs.
- [ ] Overview: workspace and window-overview animations (Hyprland
  workspace slide bezier plus a hyprexpo-style overview, or our own QML
  overview from window screenshots via hyprland-toplevel-export).

### M7. Terminal polish (recordings design)
- [ ] Tabs with per-pane glyphs, splits with drag ratios, Agents sidebar,
  chat view for Claude Code / Codex (JSON streams), and a `>_` toggle to
  the TUI on the same session id.
- [ ] Themes, a transparency slider (the shared glass value), font
  settings.

### M8. ISO and VM boot
- [ ] An archiso profile: our repo, Hyprland, Quickshell, apps, greeter
  (greetd + our QML greeter), Calamares or archinstall config, btrfs and
  snapper.
- [ ] CI builds the ISO (< 3 GB) and boots it in QEMU to the desktop; boot
  time, idle RAM and installed size go in the report.

### M9. Extras (user's)
- [ ] Droppy parity on the notch and lock screen with real MPRIS media:
  - the inventory from Droppy's docs: shelf and file drop, media controls,
    lyrics, HUDs (volume, brightness, battery, caps lock), clipboard,
    calendar, lock-screen media, and more;
  - each feature checked off or marked "not possible on Linux" with the
    reason.
- [ ] Windows 11-style management:
  - snap layouts on maximise-button hover and Super+Z (halves, thirds,
    quarters, 2/3+1/3 and grids);
  - snap zones while dragging, snap groups restored together;
  - optional auto-arrange per workspace (Hyprland dwindle/master toggled
    from the menu bar).
- [ ] Suggested extras only after approval (§4 list).

## 5b. Honest scope for one 5-hour window
At about 40% usage already spent, one window realistically covers
**M1b, M0, M2 and M3 (core)**, with screenshots. M4–M9 are weeks of work:
they continue across sessions from HANDOFF.md, in the order above. Each
milestone report states what is done, untested or stubbed. Nothing is
claimed without a VM run.

## 6. Risks
- 100–150 MB is not reachable with any GPU-composited QML shell.
- Hyprland's plugin ABI breaks each release (pin it and rebuild).
- Quickshell is young (pin it).
- Global menu on Wayland needs apps exporting DBusMenu (Qt and GTK apps
  with appmenu modules). Some apps never will.
- llvmpipe in CI inflates RAM; real-GPU numbers need the user's machine.

## 7. pearOS repos (26, licences checked 2026-11-26)
- **GPL-3.0:** liquid-gel, iso, pafari, pearos-themesw, pearos-apps-bundle,
  pearos-sounds.
- **MIT:** fastfetch and neofetch forks.
- **NOASSERTION:** bootloader, theme-switcher.
- **No licence (reference only):** pkgbuilds root (inside it: the dock is
  GPL-2.0+, window-borders GPL-3, the notch "custom"), settings,
  keyboard-shortcuts, icons (Apple recreations: never use), artwork,
  filesystem, installers, calamares config, plymouth, muternvf,
  package-repository, livecd, slimc0re-settings.

## 8. Decisions (user, 2026-11-26)
1. **Stack:** Hyprland + Quickshell (option c). Plasma/KWin components from
   the brief become Quickshell QML components. pearOS QML and liquid-gel
   are reused or rewritten into our GPL glass plugin.
2. **RAM budget:** ≤250 MB idle (`free -m` used, 5 min after login), with
   150 MB as the stretch goal. Disk 2–3 GB and ISO < 3 GB stay as written.
3. **Base:** Arch Linux + archiso. Rollback via btrfs + snapper. The Fedora
   bootc base retires.
4. **AI provider for "Search or Ask":** sign in with existing
   subscriptions: Claude (the `claude` CLI, logged in with the user's Claude
   account) and ChatGPT (the `codex` CLI, logged in with ChatGPT). Run them
   headless through their documented JSON modes, as the terminal's chat view
   already does. Pluggable, so more providers (Gemini CLI, OpenCode, API
   keys) can be added. Only official CLIs and their own login flows; never
   scraping web sessions.

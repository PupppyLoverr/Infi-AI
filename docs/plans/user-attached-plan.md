# Solyra OS — One-Shot Master Plan v2 (xnulinux Dock/Top Bar · Cupertino Control Center/Finder/Agent · Terminal merge)

Branch: `devin/1795794769-macos27-droppy-unified` (head `5a2992c`). v1 plan kept at `/Users/devin/repos/plan-v1.md`.
Research checkouts: `/Users/devin/repos/refs-research/xnulinux`, `/Users/devin/repos/refs-research/cupertino`, zeron icons/fonts (already ported).

---

## 0. Research Findings

### 0.1 `AFK-surf/xnulinux` — what is in it
- It is **not** a desktop clone. It loads Apple's open-source XNU kernel as a Linux kernel module (`xnu.ko` + GPL glue `fki.ko`) and boots a **real macOS 27.2.1 system disk made by Apple's installer**. The Dock, menu bar, Finder, Terminal and Safari in its screenshots are Apple's own binaries (`Dock.app`, `WindowServer`, `Finder.app`).
- Repo contents: `xnu-ko/` (APSL loader), `fki-ko/` + `linux/drivers/fki` (GPL foreign-kernel interface), `xnu-patches/` (LINUX64 machine layer), `kexts/` (Xnu CPU/Disk/Display/Input/Ethernet/DRM/Metal), `gpu/` (metal2vulkan, Venus, xnudrmd), `scripts/` build/boot, `tools/hosted` (fb2png, input), `docs/` (hosted-service, production, smp, free-kc), 7 screenshots.
- Requirements: arm64 + PAC, 4K pages, ≥16 GB RAM (8 GB reserved), a Mac with Xcode, your own macOS 27 install. No x86, no <3 GB, no any-PC.
- **What we can take:** zero UI code exists to port (no Apple binaries are in the repo, and copying Apple's Dock/menu bar is not legal). What we take is the **pixel reference**: its screenshots are genuine macOS 27 Golden Gate Dock and menu bar at 1280×800.
  - `refs-research/xnulinux/docs/images/desktop.jpg` — Dock + menu bar + desktop widgets (calendar, weather, photos).
  - `refs-research/xnulinux/docs/images/macos-and-linux.jpg` — Dock + menu bar + two Terminal windows.
  - `refs-research/xnulinux/docs/images/dgx-spark*.jpg`, `safari-*.jpg` — more Dock/bar/window chrome.
  - Copied into the repo as `references/xnu/` so every step can check against them.
- **macOS 27 Dock as seen in xnulinux (target):** one wide, low-contrast frosted bar (light grey tint ~14–18 % white, *very* little refraction, faint 0.5 px light rim, no heavy specular), ~62 px tall at 1280×800, icons ~46 px with ~6 px gaps, continuous corners ~18 px, thin vertical separators before Recents and before Downloads/Trash, tiny 4 px grey running dots under the icon, no visible shadow ring.
- **macOS 27 menu bar as seen in xnulinux (target):** fully transparent (no tint strip), ~24 px tall at 1280×800, Apple logo, **bold** app name, regular-weight menus with soft text shadow; right side: Control Center glyph, Spotlight, (optional `<·>` screen-share glyph), clock `Sun 22 Nov 6:22 PM`. No capsule backgrounds unless a menu is open.
- **macOS 27 Terminal as seen in xnulinux:** dark slate `#1e2733`-ish body, title-bar traffic lights, centred title `xnu — macOS — zsh -i — 84×32`, green bold prompt.

### 0.2 `orafaelfragoso/cupertino` — what is in it
A from-scratch Rust Wayland desktop shipped as an Arch distro (GPL-3.0-or-later; compatible with our `apps/` GPL-3.0-only). ~85 crates:

| Area | Crates | Take? |
|---|---|---|
| Compositor & render | `server*`, `wm`, `windowmanager`, `scene`, `gpu*`, `glass`, `effects`, `animation`, `raster-math` | Glass recipe values only (already partly ported). Keep Hyprland. |
| Toolkit | `kit`, `kit-shell`, `ui-core/controls/render/text/wayland/files/abi/test` | Needed only if we port Files/Control Center source verbatim (see Decision D1). |
| **Control panel** | `controlcenter` (9.1k lines) | **YES — exact clone.** 2 columns × 140 pt, rows 64 pt, 12 pt gaps, 292×444 grid, window 308×500 placed 6 pt from right / 12 pt below bar; capsules (Wi-Fi, Bluetooth, Microphone) with 36 pt badge 14 pt in, 13 pt semibold title / 11 pt state; Now Playing 140×140 r32 (40 pt art, buttons at 30/70/110 × 111); circles (Dock hiding, Screen lock, Mute, Keyboard backlight); Focus capsule; Display & Sound r24 with 4 pt tracks at 42 pt; Edit Controls 94×24 capsule 16 pt below; detail views (64 pt header, 28 pt heading, 32 pt rows ≤9) for Wi-Fi networks, Bluetooth devices, display/keyboard backlight, sound outputs; Edit mode (content 0.55, 18 pt remove marks, chips row, Cancel/Done, persisted order); spring open, 0.18 s close; Reduce Transparency/Increase Contrast fallbacks; 1 s refresh while open; volume throttle 16.7 ms. |
| **File manager** | `files` (21.4k), `ui-files`, `trash`, `shell-dialogs`, `shell-desktop` | **YES — exact clone.** Sidebar Favorites/Locations/Tags with blue SF-style glyphs; toolbar: back/forward capsule, title, view group (icon/list/column/gallery), Group-By, Share/Tags/More capsule, Search circle; icon grid with blue folders; one-process-per-session (`org.freedesktop.FileManager1`, `ShowItems`), Trash spec 1.0, removable devices (mount/unmount/busy), tags, rename, rubber band, context menus, gallery view, progress window, open/save portal dialog, desktop icons grid. |
| **Agent control** | `control`, `control-model`, `cli` | **YES.** Desktop-control service: `--serve`, `--session`, `--mcp` (MCP over stdio) for output-scoped observe / capture / pointer+keyboard input with compositor-granted permissions. Port as `solyra-control` on Hyprland IPC + `grim` + `wtype`/virtual-pointer, exposing the same JSON/MCP tool set so AI agents (Terminal Agent mode, Island Coding-Agent activity) can drive the desktop. |
| **Session agent** | `shell-agent` | **YES.** Log Out / Restart / Shut Down confirmation panels with 60 s countdown, power-key panel, "app is preventing logout" panel, low-battery notifications (20/10/5 %), login items, reopen apps from last session, session menu state for the Apple/system menu. |
| Launcher | `launcher` | Take: calculator + unit conversion (offline parser), persisted frecency, preview pane with actions, application browser grid (Launchpad-style, opened from Dock). |
| Overview / switcher | `shell-overview`, dock switcher panel | Take: Mission Control with desktops band, App Exposé; Cmd/Alt+Tab switcher panel. |
| OSD | `osd` | Take: 200×200 pt OSD, 16-segment meter for volume/mute/brightness/kbd backlight/mic (used when the notch HUD is disabled). |
| Screenshot | `screenshot` | Take: crosshair overlay, toolbar (screen/window/selection, Options), floating thumbnail, `Screenshot <date> at <time>.png`, clipboard, colour picker. |
| Notifications | `notifications` | Take: grouping by app, single banner + waiting slot, hover hold, swipe-to-dismiss, DND. |
| Login | `login`, `login-ipc`, `sessiond` | Take: greeter user picker, power menu, glass letters, treated wallpaper (blend with our music-first lock). |
| Menu bar | `menubar`, `menu-model` | Take behaviour only: press-drag-release, sticky click, hover-switch, type-ahead, submenu dwell, DBusMenu/global menus for Electron/Qt, Recent Items, clock options (seconds, weekday, date, AM/PM, flashing separators). Visual comes from xnulinux. |
| Daemons | `netd`, `powerd`, `btd`, `audiod`, `radios`, `portal`, `prompts`, `defaults` | Defer: lighter replacements for NetworkManager/UPower/BlueZ (RAM win) are a later phase; keep current services, adopt `prompts` (polkit/keyring prompts in our glass). |
| Assets | `cursor`, `icons`, `assets`, `theme` | Take: procedural cursor theme (XCursor + hyprcursor), theme tokens / motion catalogue. |
| Settings | `settings` (20k) | Take pane table & sidebar order for `solyra-settings` (Appearance, Notifications, Sound, Focus, Lock Screen, Privacy, Search…). |

---

## 1. v1 Plan — Status (ticked) and what gets revamped

### Phase 1 — Unified branch & assets
- [x] Unified branch from `origin/main`, all 32 `references/` + `docs/reference/` + `docs/ref/` images
- [x] 5 × 4K Unsplash wallpapers + Galaxy Space, IPC wallpaper switching
- [x] `solyra-app`, packages, PKGBUILD, icons, warm CI cache
- [x] **Revamp:** add `references/xnu/` (xnulinux) and `references/cupertino/` (Cupertino screenshots) to the reference map

### Phase 2 — UI/UX & features
- [x] 2.1 Liquid Glass + compositor (Cupertino module/dock recipe, Hyprland acrylic) — **Revamp: glass is too strong → tone down (see §2 Step 1)**
- [x] 2.2 Top bar, status menus, Spotlight, Control Center, notification suppression — **Revamp: bar → xnulinux; Control Center → exact Cupertino**
- [x] 2.3 Dock + Downloads stack, Finder, Terminal — **Revamp: Dock → xnulinux; Finder → exact Cupertino; Terminal → merged app**
- [x] 2.4 Droppy Island core, Tri-Island, Hover Peek, AirPods HUD, Reply — **Revamp: much smaller idle notch**
- [x] 2.5 Media + Live Lyrics + Queue
- [x] 2.6 File Tray (6 capabilities + Basket) + Clipboard
- [x] 2.7 Calendar Glance + Widgets suite
- [x] 2.8 3-mode lock screen + PAM — plus music-first lock with swipe-up Sign-In page
- [x] Geist/Geist Mono OS-wide + 45 Solar Pro icons from zeron — **Revamp: heavier weights**

### Phase 3–5 — Verification
- [x] qmlcheck, Rust builds, packages; 53-shot QEMU/KVM VM run
- [x] Disk 2,922 MB (< 3 GB), boot 12.8 s
- [ ] Idle RAM 150–200 MB — not proven (VM uses software llvmpipe: 1,947 MB)
- [ ] VM CI job still marked failed (budget gate on llvmpipe RAM) — classify/fix
- [ ] Interactive swipe-up test (mouse/trackpad/touch), not only IPC
- [ ] PR (waiting for your approval)

---

## 2. v2 One-Shot Execution Plan
Rule for every step: **open the listed reference images first → implement → `tools/qmlcheck.sh` / `cargo test` → compare screenshot against the same refs**.

### Step 1 — Glass: subtler, macOS 27 real-Dock level  *(refs: `references/xnu/desktop.jpg`, `references/cupertino/screenshot-control-panel.jpg`, `references/01_liquid_glass_pill.jpg`)*
- `shell/hypr/hyprland.lua`: refraction 38→~14, bulb 26→~12, clarity 0.62→~0.80, vibrancy 0.44→~0.22, chromatic aberration off/≤0.15; blur size/passes tuned so wallpaper detail stays readable.
- `shell/qs/Glass.qml`: lower specular top line (≤0.18 alpha), drop the dark side hairlines on `dock`, softer ring shadow; keep `module` for Control Center tiles (Cupertino), new `bar`-less transparent mode.
- Same values mirrored in `apps/terminal/ui/glass.json` and Finder.

### Step 2 — Dock = xnulinux (real macOS 27 Dock)  *(refs: `references/xnu/desktop.jpg`, `references/xnu/macos-and-linux.jpg`)*
- `Dock.qml`/`DockItem.qml`: ~62 px bar at 1280×800, 46 px icons, 6 px gaps, r≈18 continuous corners, light-grey low-contrast frosted fill, 0.5 px rim, no heavy shadow; separators before Recents and Downloads/Trash; 4 px grey running dots 2 px below icon.
- Keep magnification (cosine ramp, off by default like macOS), auto-hide, bounce (Cupertino timings: 710 ms parabola, attention hops 500/326/212 ms), badges (23 pt red), minimized thumbnails, Downloads stack (Cupertino placement: pointer 30×11, 14 pt above bar), context menus, Force Quit dialog, app switcher panel.

### Step 3 — Top bar = xnulinux, "changed up slightly"  *(refs: `references/xnu/desktop.jpg`, `references/xnu/macos-and-linux.jpg`, `docs/reference/ref-*.png`)*
- Fully transparent, 24 px at 1280×800 (scales with output), soft text shadow; Solyra logo, **bold** app name, Medium-weight menus.
- Right cluster order: (privacy dot) · Wi-Fi · Battery · Spotlight · Control Center · clock `Sat 28 Nov 3:00 PM`.
- "Slight change" vs macOS: 6 px pill highlight on hover (not only open), Now-Playing mini glyph when the notch is hidden, clock options from Cupertino (seconds/weekday/date/AM-PM, flashing separators).
- Behaviour from Cupertino `menubar`: press-drag-release, sticky click, hover switching, type-ahead, submenu dwell, Recent Items, global menus via DBusMenu.

### Step 4 — Control Center = exact Cupertino  *(refs: `references/cupertino/screenshot-control-panel.jpg`, `crates/cupertino-controlcenter/README.md` "Values applied")*
- Rebuild `ControlCenter.qml` on Cupertino's grid & values (§0.2 table): Wi-Fi/Bluetooth/Microphone capsules, Now Playing 140×140, Dock-hiding/Screen-lock/Mute/Keyboard-backlight circles, Focus capsule, Display & Sound r24, Edit Controls.
- Detail views (Wi-Fi networks via `nmcli`, Bluetooth via `bluetoothctl`, sound outputs via `wpctl`, display/keyboard backlight via `brightnessctl`) with Cupertino detail spring.
- Edit Controls mode with persisted order in `~/.config/solyra/controlcenter.json`.

### Step 5 — Finder = exact Cupertino Files  *(refs: `references/cupertino/screenshot-desktop.jpg`, `crates/cupertino-files/README.md`)*
- `apps/solyra-files`: sidebar (Favorites: home, Applications, Desktop, Documents, Downloads · Locations: computer, removable devices, Trash · Tags), toolbar (back/forward capsule, title, icon/list/column/gallery group, Group-By, Share/Tags/More capsule, Search), blue folder icons, gallery view, rubber band, rename, context menus, progress window, Trash spec, mount/unmount, `org.freedesktop.FileManager1` single-instance, desktop icons grid (`shell-desktop`).
- Port strategy per Decision D1.

### Step 6 — Agent control + session agent + extras from Cupertino
- `solyra-control` (Rust): `--serve` / `--session` / `--mcp` desktop-control (observe windows via Hyprland IPC, capture via `grim`, input via virtual pointer/keyboard), permission prompt in our glass, wired to Terminal Agent mode and the Island Coding-Agent activity.
- Session agent: Log Out / Restart / Shut Down panels with 60 s countdown, power-key panel, blocking-app panel, low-battery notifications, login items, reopen last session's apps.
- Launcher additions (calculator, unit conversion, frecency, app-browser grid), Overview desktops band + App Exposé, Alt+Tab switcher, screenshot tool (crosshair, toolbar, thumbnail, colour picker), OSD fallback, notification grouping/swipe/DND, procedural cursor theme, polkit/keyring prompts.

### Step 7 — Idle notch much smaller  *(refs: `references/06_droppy_media_compact.png`, `references/10_droppy_media_compact_hover.png`, `references/02_apple_dynamic_island_media.jpg`)*
- Idle 126×25 → **~88×20** (hover 100×22), r = h/2, 4 px from top; compact media keeps its own width; all expanded/HUD modes unchanged.

### Step 8 — Heavier font
- Geist body Regular→**Medium (500)**, labels/menus Medium, titles/app name **SemiBold/Bold**, clock SemiBold; Geist Mono Medium in Terminal. One place: `Theme.qml` weight tokens (`wBody`, `wLabel`, `wTitle`), applied everywhere `Font.Normal` is used; fontconfig weight default for GTK/Qt apps.

### Step 9 — Terminal = both recordings merged into one app  *(refs: `docs/ref/terminal-a_03.png`, `terminal-a_06.png`, `terminal-b_02.png`, `terminal-b_03.png`, `terminal-b_04.png`, `references/xnu/macos-and-linux.jpg`)*
- **Terminal mode (terminal-a):** traffic lights, sidebar toggle, tab-group pills with app glyphs, `+` new tab, split-right / split-down buttons; rounded inner panes (r≈12, 1 px rim) on tinted glass; Claude-Code-style header block support; bottom status line (`bypass permissions` / agent count); focused-pane highlight.
- **Agent mode (terminal-b):** near-black `#141414` body, title + workspace tag, right-aligned user bubbles (r≈14), "Thinking…" state, bottom composer (r≈16) with `Auto` · model · effort pickers and round stop/send button; `>_` button toggles back to terminal mode in the same window/tab.
- One app (`solyra-term`): each tab is either a shell or an agent session; agent sessions run a CLI agent (claude / codex / any configured command) in a PTY and render its stream as chat; split panes can mix both; uses `solyra-control` for desktop actions.
- Fix: font Geist Mono Medium, selection, scrollback, copy/paste, resize, OSC titles, true colour, ligatures off by default.

### Step 10 — Verification & fix loop
1. `tools/qmlcheck.sh`, `bash -n tools/vm/*.sh`, `cargo test --locked` for `apps/`.
2. VM scenario adds: `20a-dock-xnu`, `20b-topbar-xnu`, `21a-cc-cupertino`, `21b-cc-wifi-detail`, `21c-cc-edit`, `22a-finder-icon`, `22b-finder-list`, `22c-finder-gallery`, `23a-terminal-splits`, `23b-terminal-agent`, `24a-logout-panel`, `24b-switcher`, `24c-screenshot-tool`, `25-idle-notch-small`, plus all existing 53 shots.
3. Fix VM CI gate: report llvmpipe RAM separately; measure idle RAM with virgl/GPU where available.
4. Compare every shot with its refs (xnu Dock/bar, Cupertino CC/Finder, terminal-a/b, Droppy pack); fix and rerun until they match.
5. Send screenshots; open PR only after your approval.

---

## 3. Decisions needed
- **D1 — Port strategy for Cupertino Control Center & Finder.** (a) *Rewrite* in our stack (QML Control Center, our Rust `solyra-files`) using Cupertino's exact values, behaviours and icons — light, fits Hyprland/Quickshell, same look. (b) *Port Cupertino's Rust source verbatim* (`cupertino-files`, `cupertino-controlcenter` + CupertinoKit/kit-shell, ~60k+ lines) — closest code parity but adds its toolkit and more RAM/disk. Recommended: (a) for Control Center, (b)-lite for Finder (port `ui-files`/`files` model + `trash` logic into `solyra-files`, keep our renderer).
- **D2 — Licensing.** Porting Cupertino code is fine under GPL-3.0 (our `apps/` are GPL-3.0-only); ported files keep Cupertino's copyright notice. No xnulinux code is used (only screenshots as visual reference).

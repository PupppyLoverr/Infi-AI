# Solyra OS — One-Shot Master Plan (macOS 27 Golden Gate + Full Droppy Parity)

**Repos on disk**
- Target unified repo (`origin/main` base, commit `9553c43`): `/Users/devin/repos/Infi-AI`
- Source feature repo (`origin/antigravity/macos27-droppy-liquid-glass`, commit `5d6a59b`): `/Users/devin/repos/Infi-AI-antigravity`
- Execution mode upon user approval: **fully autonomous end-to-end without stopping** until every feature, UI/UX surface, Liquid Glass optical check across all 4K wallpapers, live QEMU/KVM VM screenshot run, and pixel-by-pixel reference audit/fix loop passes with zero QML errors and zero visual/functional regressions.

---

## 1. Branch Synthesis: Best of Each Branch

| Subsystem | Base Source | What We Keep / Port | Target Files in `/Users/devin/repos/Infi-AI` |
|---|---|---|---|
| **Liquid Glass Engine & Compositor** | `origin/main` + Golden Gate optical upgrade | Per-card `GlassWindow` Wayland layer surfaces so Hyprland `variant=8` acrylic lensing bends backdrop edges on every individual card; `rounding=24`; 3 materials (`clear`, `frosted`, `smoke`); upgraded specular inner/outer rim + corner glint matching `01_liquid_glass_pill.jpg`; `ELECTRON_OZONE_PLATFORM_HINT=auto` + Flatpak `XDG_DATA_DIRS` from `antigravity`. | `shell/hypr/hyprland.lua`, `shell/qs/Glass.qml`, `shell/qs/GlassWindow.qml`, `shell/qs/Theme.qml` |
| **Top Menu Bar & Status Menus** | `origin/main` | Transparent macOS 27 menu bar, bold app name, per-app menus, glass capsules on active items, real UPower/power-profiles `BatteryMenu.qml`, real `nmcli` password-join `WifiMenu.qml`, `FocusMenu.qml`, `StatusMenu.qml`, `ScreenCorners.qml`, Melbourne AEDT/AEST timezone. | `shell/qs/Bar.qml`, `shell/qs/MenuTitle.qml`, `shell/qs/BatteryMenu.qml`, `shell/qs/WifiMenu.qml`, `shell/qs/FocusMenu.qml`, `shell/qs/StatusMenu.qml`, `shell/qs/ScreenCorners.qml` |
| **Control Center** | `origin/main` | Separate floating `GlassWindow` pill tiles (Wi-Fi, Bluetooth, Airplane Mode, Now Playing, Snap Layouts, Screen Recording, Dark Mode, Focus, Display & Sound sliders, outputs & per-app volume, Glass clear→tinted slider, Wallpaper switcher, Desktop Widgets toggle, Edit Controls). | `shell/qs/ControlCenter.qml` |
| **Spotlight** | `origin/main` | Smoke capsule springing over the notch (`GlassWindow` `kind: "smoke"`) + separate Liquid Glass results panel (`Applications ^1`, `Files ^2`, `Actions ^3`, `Clipboard ^4`, Ask row, whisper.cpp dictation) + notification suppression while open. | `shell/qs/Spotlight.qml`, `shell/qs/SearchModel.qml` |
| **Notification Center & Banners** | `origin/main` + overlap fix | Panel-less macOS 27 Notification Center with Edit Widgets; suppress/defer banners while Spotlight, Control Center, or an expanded Island panel is open. | `shell/qs/NotificationCenter.qml`, `shell/qs/Notifications.qml` |
| **Dock & Downloads Stack** | `origin/main` proportions + `antigravity` stack/hover | Exact-size `GlassWindow` dock (50px icons, slim padding, thin dividers, running dots, recent apps, minimized thumbnails) + `antigravity`'s Downloads 3×2 Liquid Glass stack popover (`15d`) + toggleable wave magnification (`15e`). | `shell/qs/Dock.qml`, `shell/qs/DockItem.qml` |
| **Finder (`solyra-files`)** | `origin/main` | macOS 26/27 translucent floating sidebar over near-solid content area, concentric 24px corners, no duplicate breadcrumb row, live item counts in every view, selection-only action bar. | `apps/solyra-files/src/main.rs` |
| **Terminal (`solyra-term`)** | `origin/main` (shared) | Native `zgx` terminal with macOS traffic lights (`lights.luau` with close/minimize/zoom hover glyphs) verified against `docs/ref/terminal-*.png`. | `apps/terminal/src/main.rs`, `apps/terminal/ui/app.luau`, `apps/terminal/ui/components/lights.luau` |
| **Droppy Dynamic Island & Live Activities** | `antigravity` + `main` opacity | Pure-black opaque expanded island (zero menu-title ghosting), Tri-Island ("Three Things At Once": Calendar + Coding Agent + Focus Timer), 2-row Hover Peek, concave-ear AirPods Pro Battery Ring HUD, Reply from Island inline messaging, Notchface, Volume/Brightness HUDs. | `shell/qs/Island.qml`, `shell/qs/Equaliser.qml`, `shell/qs/shell.qml` |
| **Droppy Media, Live Lyrics & Queue** | `antigravity` | Expanded Media controls, real MPRIS + `lrclib.net` Live Synced Lyrics `[L]` vertical drawer, Playing Next Queue (`≡`) drawer, output picker. | `shell/qs/MediaView.qml`, `shell/qs/LyricsService.qml` |
| **Droppy File Tray & Clipboard** | `antigravity` | All 6 File Tray capabilities (Zip/unzip, Extract & Shrink with green-check compression toast, New Folders, Folder Drops, Pinned folders, Tracked folders, rich previews, blue multi-select, Floating Basket, tray badge) + Rich Clipboard Manager (`Video`, `#FF9F43` Color swatch, `Link`, `Text`, `Clipboard`/`Work`/`Personal` pills, Incognito, dark Copy/Pin/Delete context menu). | `shell/qs/ShelfView.qml`, `shell/qs/ShelfModel.qml`, `shell/qs/FloatingBasket.qml`, `shell/qs/ClipView.qml`, `shell/qs/ClipHistory.qml` |
| **Droppy Calendar Glance & 3-Col Widgets** | `antigravity` + glass-fade polish | Two-column Calendar Glance with dark-to-frosted-glass bottom fade (`22`), 3-Column Multi-Widget Dashboard (Now Playing + Calendar + Blue Sky Hourly Weather), Widget Customizer (`−` badges & bottom picker), Pomodoro interactive tick-mark dial (`10..40`, `25:00`), Storage Cleaner 5-row dot-matrix LED bar, Per-App PipeWire Audio Mixer with unity tick, AI Usage radial rings (Claude & Codex). | `shell/qs/CalendarGlance.qml`, `shell/qs/WidgetsDashboard.qml`, `shell/qs/PomodoroWidget.qml`, `shell/qs/StorageCleanerWidget.qml`, `shell/qs/AudioMixerWidget.qml`, `shell/qs/UsageWidget.qml`, `shell/qs/DesktopWidgets.qml` |
| **Droppy 3-Mode Ambient Lock Screen** | `antigravity` + depth/switcher polish | Fullscreen ambient Lock Screen with PAM auth and 3-mode switcher pill: Mode 1 Full Art & Vision Clock Depth, Mode 2 Live Synced Lyrics, Mode 3 Playing Next Queue. | `shell/qs/Lock.qml` |
| **Launcher, Icons, Refs & CI** | `antigravity` + `main` | `iso/airootfs/usr/local/bin/solyra-app`, all new SVGs from both branches in `shell/qs/icons/`, `pkg/solyra-shell/PKGBUILD`, `iso/packages.solyra`, `references/` (32 files), warm CI binary cache (`actions/cache@v4` in `.github/workflows/vm.yml`), `tools/qmlcheck.sh`. | `iso/`, `pkg/`, `references/`, `.github/workflows/vm.yml`, `tools/qmlcheck.sh`, `tools/vm/` |

---

## 2. 4K Unsplash Wallpaper Suite for macOS 27 Golden Gate Liquid Glass Testing

All 5 wallpapers are verified 4K landscape JPEGs (plus existing `galaxy-1920x1080.png` and `solyra-dusk-1920x1200.png`), bundled into `/Users/devin/repos/Infi-AI/assets/wallpapers/` and wired into wallpaper switching & VM glass tests:

| # | Local Asset Path (in `Infi-AI/assets/wallpapers/`) | Source Unsplash 4K URL | Optical Stress-Test Purpose |
|---|---|---|---|
| 1 | `unsplash-twilight-skyline-4k.jpg` *(User's pick — Default)* | `https://images.unsplash.com/photo-1762430259780-28ffac74916c?w=3840&auto=format&fit=crop&q=95` | Dark sky at top (menu bar/notch), glowing amber skyline across middle (Spotlight/Control Center/Finder refraction), shimmering water at bottom (Dock lensing). |
| 2 | `unsplash-alpine-dawn-4k.jpg` | `https://images.unsplash.com/photo-1506905925346-21bda4d32df4?w=3840&h=2160&fit=crop&auto=format&q=95` | Crisp alpine ridges + sunrise cloud deck; verifies high-frequency edge refraction and colour vibrancy. |
| 3 | `unsplash-liquid-abstract-4k.jpg` | `https://images.unsplash.com/photo-1557672172-298e090bd0f1?w=3840&h=2160&fit=crop&auto=format&q=95` | High-contrast cyan/magenta fluid veins; verifies chromatic aberration fringe & curved acrylic rim bending. |
| 4 | `unsplash-golden-valley-4k.jpg` | `https://images.unsplash.com/photo-1500534314209-a25ddb2bd429?w=3840&h=2160&fit=crop&auto=format&q=95` | Warm apricot sky & layered sage ridges; matches macOS 27 Golden Gate warm light-glass reference (`01_liquid_glass_pill.jpg`, `ref-3.png`). |
| 5 | `unsplash-light-arch-4k.jpg` | `https://images.unsplash.com/photo-1487958449943-2429e8be8625?w=3840&h=2160&fit=crop&auto=format&q=95` | Bright white/silver geometric glass facade; verifies bright-backdrop glass rim contrast, line bending through acrylic lens, and adaptive ink legibility. |
| 6 | `galaxy-1920x1080.png` *(from `antigravity`)* | `references/00_galaxy_space_wallpaper.jpg` | Deep-space orange/blue accretion disk; verifies Droppy pure-black island & luminous glass bleed. |
| 7 | `solyra-dusk-1920x1200.png` *(from `main`)* | Existing in `origin/main` | Smooth dusk-coast gradient; verifies subtle frosted/clear glass transitions. |

---

## 3. Complete Reference Map (Every Single Reference Image — 44 Total)

**Rule during execution:** Before implementing or tuning ANY component in Phase 2, open and inspect its exact reference image(s) listed below using `read`, implement to match pixel geometry/optics/behavior, and compare against the same reference image(s) in Phase 4.

### A. Droppy + macOS 27 Golden Gate Pack (`references/`, 32 images)

| Ref File (under `/Users/devin/repos/Infi-AI/references/`) | Target Component(s) & Files | Required Parity Spec |
|---|---|---|
| `00_galaxy_space_wallpaper.jpg` | `assets/wallpapers/galaxy-1920x1080.png`, `shell/qs/shell.qml` | Secondary dark cosmic test wallpaper for glass refraction. |
| `01_liquid_glass_pill.jpg` | `shell/qs/Glass.qml`, `shell/qs/GlassWindow.qml`, `shell/hypr/hyprland.lua` | Ultra-clear macOS 27 Golden Gate pill optics: curved refractive edge bevel, top-right specular reflection glint, luminous thin 1px rim, high clarity without muddy grey fill. |
| `02_apple_dynamic_island_media.jpg` | `shell/qs/Island.qml`, `shell/qs/MediaView.qml` | Pure-black expanded Dynamic Island media card: album art left, title/artist + equalizer right, scrub bar with timestamps, crisp play/pause/prev/next + output icon. |
| `03_droppy_clipboard_copy_menu.jpg` | `shell/qs/ClipView.qml`, `shell/qs/ClipHistory.qml` | Clipboard cards (`Video`, `#FF9F43` Color swatch with RGB footer, `Link`, `Text`) + dark floating Copy/Paste/Pin/Delete context menu. |
| `04_droppy_lock_centered_cover.jpg` | `shell/qs/Lock.qml` | Fullscreen Lock Screen Mode 1 centered cover + clean single-line clock & translucent Liquid Glass transport controls. |
| `05_droppy_calendar_events_grid.png` | `shell/qs/CalendarGlance.qml` | Two-column Calendar Glance: left = `DAY DD W##` + `+` / calendar buttons + coloured event cards with vertical accent bars; right = month header + compact 7-col grid with red today pill. |
| `06_droppy_media_compact.png` | `shell/qs/Island.qml`, `shell/qs/Equaliser.qml` | Compact live media notch: small rounded album art on left, animated equalizer bars on right, pure-black pill. |
| `07_droppy_airpods_battery_ring_hud.png` | `shell/qs/Island.qml`, `shell/qs/icons/airpods.svg` | Concave-ear top-attached HUD: AirPods icon left, "Connected / AirPods Pro" center, green circular battery progress ring with numeric percentage (`80`) right. |
| `08_droppy_widgets_customizer_pomodoro.jpg` | `shell/qs/WidgetsDashboard.qml`, `shell/qs/PomodoroWidget.qml` | Widget Customizer mode: red/grey `−` remove badges on active widgets, bottom widget picker tray to add/reorder, live Pomodoro dial tile. |
| `09_droppy_clipboard_pin_menu.jpg` | `shell/qs/ClipView.qml`, `shell/qs/ClipHistory.qml` | Clipboard category pills (`Clipboard`, `Work`, `Personal`), Incognito pill, pin/favorite menu & pinned badge state. |
| `10_droppy_media_compact_hover.png` | `shell/qs/Island.qml` | Compact media hover interaction: smooth spring expansion & subtle highlight/peek on pointer hover. |
| `11_droppy_media_lyrics_drawer.jpg` | `shell/qs/MediaView.qml`, `shell/qs/LyricsService.qml` | Media card + toggled `[L]` Live Synced Lyrics vertical drawer (active line bold bright white, surrounding lines dimmed, smooth scroll via `lrclib.net`). |
| `12_droppy_media_playing_next_queue.jpg` | `shell/qs/MediaView.qml` | Media card + Playing Next queue drawer showing upcoming tracks with artwork/icons and durations. |
| `13_droppy_lock_split_playing_next.jpg` | `shell/qs/Lock.qml` | Fullscreen Lock Screen Mode 3: split view with large album art + controls left, "Playing Next / Continue Playing" list + shuffle/repeat right. |
| `14_droppy_lock_centered_ambient.jpg` | `shell/qs/Lock.qml` | Fullscreen Lock Screen ambient blurred album-art backdrop with subtle vignette & centered media controls. |
| `15_droppy_widgets_dashboard_3col.jpg` | `shell/qs/WidgetsDashboard.qml` | 3-column horizontal dashboard inside island: Now Playing (left) + Calendar events (middle) + vibrant blue hourly Weather card (right), with compact detached pill switcher below. |
| `16_droppy_airpods_battery_ring_hud.png` | `shell/qs/Island.qml` | Close-up verification of AirPods Battery Ring HUD proportions, ring stroke width, and concave top-ear curvature. |
| `17_droppy_notch_playing_next_queue.png` | `shell/qs/MediaView.qml` | Highlighted Queue (`≡`) toggle button state & scrollable queue rows inside notch. |
| `18_droppy_lockscreen_3mode_switcher_spec.png` | `shell/qs/Lock.qml` | Bottom-right 3-mode segmented switcher pill (`Full Art & Depth` / `Live Lyrics` / `Playing Next`) with crisp active white capsule. |
| `19_droppy_file_tray_6_capabilities_spec.png` | `shell/qs/ShelfView.qml`, `shell/qs/ShelfModel.qml` | All 6 real File Tray capabilities: (1) Zip archives & unzip in place, (2) Extract & shrink photos/PDFs, (3) New folders gathering selection, (4) Folder drops, (5) Pinned folders, (6) Tracked watched folders. |
| `20_droppy_three_things_hover_peek.png` | `shell/qs/Island.qml` | 2-row Hover Peek expansion from Tri-Island showing active task name + countdown timer + icon cleanly. |
| `21_droppy_three_things_tri_island.png` | `shell/qs/Island.qml` | Tri-Island ("Three Things At Once"): left circular ring (Calendar event), center pill (Coding agent + progress spinner), right circular ring (Focus timer). |
| `22_droppy_calendar_glance_glass.png` | `shell/qs/CalendarGlance.qml`, `shell/qs/Island.qml` | Calendar Glance dark-top-to-frosted-glass bottom fade over wallpaper + detached pill switcher below. |
| `23_droppy_storage_cleaner_widget.png` | `shell/qs/StorageCleanerWidget.qml` | Storage Cleaner: used/available header + refresh button, 5-row dot-matrix LED storage bar (color-coded Apps/Caches/System), category breakdown, "Drop an app to uninstall" + teal Sparkle Clean & Trash buttons. |
| `24_droppy_file_tray_drag_multiselect.png` | `shell/qs/ShelfView.qml`, `shell/qs/ShelfModel.qml` | Blue card highlight on selected items, multi-select checkmark toggle, drag-and-drop visual feedback, live item count badge on tray tab icon. |
| `25_droppy_file_tray_compressed_toast.png` | `shell/qs/ShelfView.qml`, `shell/qs/ShelfModel.qml` | Floating dark pill toast with green checkmark after real PDF/image compression showing original → compressed size. |
| `26_droppy_lockscreen_full_art_depth.png` | `shell/qs/Lock.qml` | Full-bleed motion/cover art with Vision Clock Depth (single-line clock + depth icon + date) and translucent Liquid Glass controls. |
| `27_droppy_per_app_audio_mixer_widget.png` | `shell/qs/AudioMixerWidget.qml` | Per-App PipeWire Audio Mixer: orange Speaker / Mic tab toggles + reset button, horizontal capsule volume sliders per app with app icon, vertical 100% unity tick line, and percentage label. |
| `28_droppy_file_tray_pure_black.png` | `shell/qs/ShelfView.qml` | Pure-black File Tray card previews: image thumbnail with dimensions, PDF card preview, live text snippet preview, folder icon with item count. |
| `29_droppy_feature_grid_reply_from_island.png` | `shell/qs/Island.qml`, `shell/qs/Lock.qml` | Lock Screen depth glyph + Reply from the Island inline message banner (sender avatar, app badge, message text, rounded input pill + blue send arrow button). |
| `30_droppy_ai_usage_rings_widget.png` | `shell/qs/UsageWidget.qml`, `shell/qs/icons/claude.svg`, `shell/qs/icons/codex.svg` | AI Usage widget: `Usage` header + settings/refresh buttons, radial progress rings for Claude (amber arc + `% left`) and Codex (red low-quota arc + blue alert badge + `% left`). |
| `31_droppy_now_playing_and_pomodoro_dial.jpg` | `shell/qs/PomodoroWidget.qml`, `shell/qs/WidgetsDashboard.qml` | Interactive Pomodoro tick-mark dial (`10 15 20 25 30 35 40` with amber pointer triangle), `Focus`/`Break` pills, `Start Timer`, audio/stopwatch toggles, large amber `25:00` readout. |

### B. Desktop, Glass, Snap & Widgets References (`docs/reference/`, 7 images)

| Ref File (under `/Users/devin/repos/Infi-AI/docs/reference/`) | Target Component(s) & Files | Required Parity Spec |
|---|---|---|
| `docs/reference/ref-1.png` | `shell/qs/Bar.qml`, `shell/qs/Island.qml`, `shell/qs/Dock.qml` | Core spatial architecture: macOS Finder-style top bar + Droppy notch center + floating Dock. |
| `docs/reference/ref-2.png` | `shell/qs/SnapLayouts.qml`, `shell/hypr/hyprland.lua` | Windows-11-style 6-zone Snap Layouts glass flyout (halves, thirds, quarters) + Aero-snap edge/corner drag. |
| `docs/reference/ref-3.png` | `shell/qs/Spotlight.qml`, `shell/qs/Dock.qml`, `shell/qs/Bar.qml` | macOS 27 Golden Gate desktop: transparent top bar, near-black smoke Spotlight capsule with specular rim, sleek floating glass dock. |
| `docs/reference/ref-4.png` | `shell/qs/Bar.qml`, `shell/qs/Glass.qml`, `apps/solyra-files/src/main.rs`, `shell/qs/DesktopWidgets.qml` | Translucent Liquid Glass context menus & popups, bright glass window chrome, desktop widgets fading back when windows open, dock proportions. |
| `docs/reference/ref-6.png` | `shell/qs/Glass.qml`, `shell/qs/Dock.qml` | Minimalist ultra-clear Liquid Glass rim & dock purity. |
| `docs/reference/ref-7.png` | `shell/qs/DesktopWidgets.qml`, `shell/qs/NotificationCenter.qml` | Clean glass widget cards (Calendar, Weather, Quick Controls, Performance/Volume) toggleable from Control Center / Notification Center. |
| `docs/reference/ref-7b-win11-start.png` | `shell/qs/Spotlight.qml` | Fast app/file/action launch ergonomics integrated into Spotlight's Applications/Files/Actions/Clipboard views. |

### C. Native Terminal References (`docs/ref/`, 5 images)

| Ref File (under `/Users/devin/repos/Infi-AI/docs/ref/`) | Target Component(s) & Files | Required Parity Spec |
|---|---|---|
| `docs/ref/terminal-a_03.png` | `apps/terminal/ui/app.luau`, `apps/terminal/ui/components/lights.luau` | macOS traffic lights + minimal glass titlebar & tab styling. |
| `docs/ref/terminal-a_06.png` | `apps/terminal/ui/app.luau` | Translucent frosted terminal body, crisp monospace typography, padding. |
| `docs/ref/terminal-b_02.png` | `apps/terminal/ui/app.luau` | Split/command readability on translucent glass backdrop. |
| `docs/ref/terminal-b_03.png` | `apps/terminal/ui/app.luau` | Accent/prompt contrast on dark & clear glass. |
| `docs/ref/terminal-b_04.png` | `apps/terminal/ui/app.luau` | Window rounding (24px concentric with Hyprland) & clean border hairline. |

---

## 4. macOS 27 Golden Gate Liquid Glass Optical Upgrade Plan

To make the glass look **indistinguishable from macOS 27 Golden Gate** (`references/01_liquid_glass_pill.jpg`, `docs/reference/ref-3.png`, `ref-4.png`) across all 5 Unsplash 4K wallpapers + Galaxy Space + Solyra Dusk:

1. **Per-Card Wayland Layer Surfaces (`GlassWindow.qml`)**:
   - Every floating glass element (Control Center tiles, Spotlight capsule & results panel, Dock, Downloads stack popover, Bar status menus, Snap Layouts, Notifications, Desktop Widgets) renders in its own exact-bounds `GlassWindow` so Hyprland's acrylic lens bends the backdrop at every individual card's rim.
2. **Hyprland 0.57.2 Acrylic Lens Tuning (`shell/hypr/hyprland.lua`)**:
   - Keep `variant = 8`, `rounding = 24`, `passes = 4`, `size = 14`, `popups = true`, `popups_ignorealpha = 0.16`.
   - Tune `acrylic`: `refraction = 32` (strong curved rim bending like `01_liquid_glass_pill.jpg`), `bulb = 24` (optical bevel width), `clarity = 0.58` (crystal clear, never muddy), `aberration = 0.022` (subtle prism dispersion at edges), `vibrancy = 0.38`, `vibrancy_darkness = 0.18`.
3. **QML Optical Specular Surface (`shell/qs/Glass.qml`)**:
   - Upgrade the specular rim to a dual-layer optical edge:
     - **Outer crisp 1px specular ring** lit strongly at top-left/top-right corners and bottom exit edge, fading along vertical sides.
     - **Inner 1.5px caustic refraction sheen** (soft diagonal/top-right curved highlight matching the curved glint in `01_liquid_glass_pill.jpg`).
     - **Adaptive light/dark tint**: on bright wallpapers (`unsplash-light-arch-4k`, `unsplash-golden-valley-4k`) keep glass airy and luminous without washing out white icons/text (subtle legibility scrim where needed).
     - Keep body alpha strictly `>= 0.17` (above `ignore_alpha = 0.16`) and shadow alpha `<= 0.15` (below `ignore_alpha = 0.16`) so shadows never blur into boxes.

---

## 5. Step-by-Step Autonomous Execution Plan

### Phase 1 — Unified Branch & Assets Setup
1. Create a clean working branch on `/Users/devin/repos/Infi-AI` from `origin/main` (`9553c43`): `devin/<ts>-macos27-droppy-unified`.
2. Copy all 32 reference images + `references/README.md` from `/Users/devin/repos/Infi-AI-antigravity/references/` into `/Users/devin/repos/Infi-AI/references/`.
3. Bundle the 5 verified 4K Unsplash wallpapers into `/Users/devin/repos/Infi-AI/assets/wallpapers/` + `galaxy-1920x1080.png` from `antigravity`, and wire them into wallpaper selection (defaulting to `unsplash-twilight-skyline-4k.jpg` with instant IPC wallpaper switching for multi-wallpaper VM glass shots).
4. Port `iso/airootfs/usr/local/bin/solyra-app`, `iso/packages.solyra` additions, `pkg/solyra-shell/PKGBUILD` additions, all `antigravity` SVG icons in `shell/qs/icons/`, and `.github/workflows/vm.yml` warm binary cache (`actions/cache@v4`, triggered on our working branch).

### Phase 2 — Ref-Checked UI/UX & Full Feature Implementation
For each step below: **(a) `read` the listed reference images first → (b) implement/merge & wire real backend behavior → (c) run `tools/qmlcheck.sh`**.

1. **Liquid Glass & Compositor** *(Refs: `01`, `ref-3`, `ref-4`, `ref-6`)*
   - Merge `hyprland.lua` (`main`'s `rounding=24`, `GlassWindow` layer rules, Melbourne TZ + `antigravity`'s Electron/Flatpak env + Golden Gate acrylic tuning).
   - Upgrade `Glass.qml` & `GlassWindow.qml` specular bevel, corner glint, and clear/frosted/smoke materials.
2. **Top Menu Bar, Status Menus, Spotlight, Control Center & Notifications** *(Refs: `ref-1`, `ref-2`, `ref-3`, `ref-4`, `ref-7b`)*
   - Keep `main`'s `Bar.qml`, `MenuTitle.qml`, `WifiMenu.qml`, `BatteryMenu.qml`, `FocusMenu.qml`, `StatusMenu.qml`, `ScreenCorners.qml`, `Spotlight.qml`, `ControlCenter.qml`, `NotificationCenter.qml`, `SnapLayouts.qml`.
   - Add wallpaper quick-switcher & desktop-widgets toggle into `ControlCenter.qml` (`GlassWindow` tiles).
   - Fix `Notifications.qml` to suppress/defer banners while Spotlight, Control Center, or an expanded Island panel is open.
3. **Dock, Downloads Stack, Finder & Terminal** *(Refs: `ref-4`, `ref-6`, `docs/ref/terminal-*.png`)*
   - Merge `antigravity`'s Downloads 3×2 Liquid Glass stack popover & optional hover wave magnification into `main`'s `GlassWindow`-based `Dock.qml` & `DockItem.qml` (preserving `main`'s exact 50px proportions, dividers, running dots, minimized thumbnails).
   - Keep `main`'s `apps/solyra-files/src/main.rs` and `apps/terminal/` (verify `cargo test --locked`).
4. **Droppy Dynamic Island Core & Live Activities** *(Refs: `02`, `06`, `07`, `10`, `16`, `20`, `21`, `29`)*
   - Port `antigravity`'s full `Island.qml` + `Equaliser.qml` + `shell.qml` IPC handlers onto `main`'s shell root (preserving `id: root`, opaque pure-black expanded island, Spotlight-over-notch handoff).
   - Verify Tri-Island (Calendar + Coding Agent + Focus Timer), 2-row Hover Peek, concave-ear AirPods Pro Battery Ring HUD, Volume/Brightness HUDs, Screen Recording activity, Notchface, and Reply from Island with real dispatch.
5. **Droppy Media + Live Synced Lyrics `[L]` + Playing Next Queue** *(Refs: `02`, `11`, `12`, `17`)*
   - Port `MediaView.qml` and `LyricsService.qml` (`lrclib.net` live sync + fallback), verify play/pause/seek/next/prev, `[L]` lyrics drawer, `≡` queue drawer, and audio output selector.
6. **Droppy File Tray (All 6 Capabilities + Floating Basket) & Rich Clipboard Manager** *(Refs: `03`, `09`, `19`, `24`, `25`, `28`)*
   - Port `ShelfView.qml`, `ShelfModel.qml`, `FloatingBasket.qml`, `ClipView.qml`, `ClipHistory.qml`.
   - Verify all 6 File Tray actions work end-to-end (real zip/unzip, real image/PDF shrink + floating green-check compression toast, new folder creation, folder drops, pinned folders, tracked folders) + multi-select blue highlight + live badge count.
   - Verify Clipboard Manager cards (`Video`, `#FF9F43` Color swatch, `Link`, `Text`), category filtering (`Clipboard`, `Work`, `Personal`), Incognito mode, and dark Copy/Pin/Delete context menu.
7. **Droppy Calendar Glance & 3-Column Widgets Suite** *(Refs: `05`, `08`, `15`, `22`, `23`, `27`, `30`, `31`, `ref-7`)*
   - Port `CalendarGlance.qml` and add the dark-top-to-frosted-glass bottom gradient matching `22_droppy_calendar_glance_glass.png`.
   - Port `WidgetsDashboard.qml`, `PomodoroWidget.qml`, `StorageCleanerWidget.qml`, `AudioMixerWidget.qml`, `UsageWidget.qml`, `DesktopWidgets.qml`.
   - Polish 3-column dashboard height/spacing (`15`), Pomodoro tick-mark dial (`31`), Storage Cleaner 5-row dot-matrix + real cleanup (`23`), PipeWire Audio Mixer with 100% unity tick (`27`), AI Usage radial rings + alert badge (`30`).
8. **Droppy 3-Mode Fullscreen Ambient Lock Screen** *(Refs: `04`, `13`, `14`, `18`, `26`, `29`)*
   - Port `Lock.qml` with PAM unlock, Mode 1 Full Art & Vision Clock Depth, Mode 2 Live Synced Lyrics, Mode 3 Playing Next Queue, and exact 3-mode switcher pill (`18`).

### Phase 3 — Local Checks + Comprehensive Live VM Run (`solyra-vm` CI)
1. Run local gates: `tools/qmlcheck.sh` (zero QML syntax/type errors), `shellcheck -S warning` on shell scripts, and Rust tests where supported.
2. Unify `tools/vm/scenario.sh` to capture **~52 live VM screenshots** covering every single surface from BOTH branches plus multi-wallpaper Liquid Glass shots:
   - `01-desktop-idle-notch.jpg` (on Twilight Skyline 4K)
   - `01b-glass-alpine-dawn-4k.jpg`, `01c-glass-liquid-abstract-4k.jpg`, `01d-glass-golden-valley-4k.jpg`, `01e-glass-light-arch-4k.jpg`, `01f-glass-galaxy-space.jpg` (Control Center + Dock + Spotlight glass tested across all 4K wallpapers!)
   - `02-notch-live-activity.jpg`, `03-island-media.jpg`, `03b-island-media-queue.jpg`
   - `04-island-clipboard.jpg`, `04b-island-clipboard-menu.jpg`
   - `05-island-shelf.jpg`, `05b-island-shelf-compressed-toast.jpg`
   - `06-island-tools.jpg`, `06a-island-calendar.jpg`, `06b-island-notchface.jpg`, `06c-island-widgets-3col.jpg`, `06d-island-widgets-customizer.jpg`, `06e-island-pomodoro.jpg`, `06f-island-storage-cleaner.jpg`, `06g-island-audio-mixer.jpg`, `06h-island-ai-usage.jpg`
   - `07-hud-volume.jpg`, `07b-tracked-downloads-on-shelf.jpg`, `07c-tri-island-three-activities.jpg`, `07d-recording-activity.jpg`, `07e-hud-airpods-battery-ring.jpg`, `07f-tri-island-hover-peek.jpg`, `07g-reply-from-island.jpg`
   - `08-spotlight.jpg`, `08b-spotlight-results.jpg`
   - `09-control-center.jpg`, `10-notification.jpg`, `10b-notification-center.jpg`, `11-snap-layouts.jpg`
   - `12-terminal.jpg`, `12b-mission-control.jpg`, `14-glass-tinted.jpg`, `15-glass-clear.jpg`
   - `15b-finder.jpg`, `15c-dock-minimized.jpg`, `15d-dock-downloads-stack.jpg`, `15e-dock-magnified-hover.jpg`
   - `16a-lock-screen-full-art-depth.jpg`, `16b-lock-screen-live-lyrics.jpg`, `16c-lock-screen-playing-next.jpg`
   - `17-web-song-playing.jpg`, `17b-web-song-notch.jpg`, `17d-web-song-control-center.jpg`, `17e-web-song-notch-lyrics.jpg`, `17f-web-song-notch-queue.jpg`
   - `18-wifi-menu.jpg`, `18b-focus-menu.jpg`, `18c-battery-menu.jpg`
3. Push to our branch, wait for `solyra-vm` CI to complete, pull the committed live screenshots and logs (`log-qs.log`, `report.txt`, `lrclib-live-check.txt`, `solyra-app-check.txt`).

### Phase 4 — Pixel-by-Pixel Reference Comparison & Autonomous Fix Loop
1. Inspect `docs/shots/vm/log-qs.log` (must have **0 QML errors/warnings**) and `report.txt` (verify RAM & disk within budget).
2. Visually inspect EVERY live VM screenshot side-by-side against its corresponding reference in `references/` and `docs/reference/` using `read`.
3. For any mismatch (glass clarity/rim/refraction, clipping, overlap, font/spacing, missing icon, broken feature, lock screen depth, calendar glass fade, etc.):
   - Fix the root cause in code.
   - Re-run `tools/qmlcheck.sh` and push for a fresh `solyra-vm` CI run.
   - Re-verify the updated screenshots against the references.
   - Repeat until **every single surface passes visual & functional parity**.

### Phase 5 — Final Delivery (Awaiting PR/Merge Approval)
- Send all final live VM screenshots (including the multi-wallpaper 4K Golden Gate Liquid Glass shots), a concise parity checklist, and wait for your explicit approval before opening a PR or merging to `origin/main`.

COSMOSOS — MASTER PROMPT v3 (UI/UX TO macOS / WINDOWS 11 LEVEL)

You are the lead engineer AND lead product designer for CosmosOS, a Linux distribution. Your job in this session: make CosmosOS look and feel like a genuine, shipped, premium desktop OS that stands next to macOS and Windows 11. It must not look like a Linux dev demo, a default egui app, or generic AI-generated UI. Everything you claim must be real, implemented, booted and screenshotted. No stubs, mocks, placeholders, TODOs, fake data, or screenshots of anything except the real booted guest.

============================================================
0. CONTEXT — WHAT ALREADY EXISTS (DO NOT REBUILD, DO NOT BREAK)
============================================================
Repo: github.com/PupppyLoverr/Infi-AI, folder cosmosos/. Main is at 3bc1478 or later. Read docs/STATUS.md, docs/ARCHITECTURE.md, docs/budgets.md, docs/ui-ux.md and docs/evidence/blitz-*.png first (10 min max).

Stack (locked, do not change):
- Debian 13 trixie, kernel 6.12, GPT plus btrfs subvolumes plus snapper, zram, en_US.UTF-8. Image built by cosmosos/image/build.sh, booted by image/run.sh in QEMU (virtio-vga, 1920x1080 via GUEST_RES).
- cosmos-compositor: our own Rust Smithay 0.7 compositor (~13.5k LOC). SSD chrome with traffic lights, workspaces 1-9, snap (13 zones, shared cosmos_ipc::SNAP_LAYOUTS), Snap Assist, zoom-button snap flyout, master-stack tiling, motion system, XWayland, fractional scaling, IPC socket, QMP drive harness. Do NOT fork cosmic-comp. Do NOT switch toolkit.
- cosmos-shell: one process of layer-shell surfaces (menubar, left floating dock pill, dynamic island pill, Search-or-Ask launcher plus Start widgets, Control Centre, notifications/approvals, snap assist, switcher).
- cosmos-uitk: egui toolkit on sctk plus a software rasterizer. Apps: terminal, files, editor, settings, monitor, agents.
- cosmos-agentd (MCP, approvals), cosmos-portal (xdg-desktop-portal backend).
- Budgets today: ~400 MiB idle used on a 2 GB VM, ~1.8 GB image.

What the latest evidence shows is STILL WRONG (fix all of these):
- Every app window is default egui neutral grey (#191919) with no rounding, cramped spacing and no visual hierarchy.
- There's no glass anywhere. The menubar is an opaque black bar and the dock pill is opaque black.
- Icons are small monochrome line glyphs, not real app icons.
- A dark blurred rectangle sits at x=0..~200px, full height, on every screenshot.
- The wallpaper is muddy and dim.
- Start panel widget cards overlap the Settings/Log out footer.
- The approval card shows a stray "{" line and a truncated path.
- Files shows only dotfiles. The editor is an empty grey slab with a dead block under it.
- Light mode doesn't fully reach apps.
- The greeter is flat black.

============================================================
1. HARD CONSTRAINTS
============================================================
- Budgets (must still pass at the end, measured with the existing measure tooling): idle RAM < 1 GB on a 2 GB VM (target: stay under 500 MiB), image <= 3.5 GB, install <= 4 GB, boot to desktop <= 10 s in KVM.
- Must render well on llvmpipe (software GL / CPU rasterizer). No effect may require a GPU. Live blur is optional and only for a GPU tier.
- Original assets only. No Apple/Microsoft icons, fonts, wallpapers or SF Symbols. Fonts: Inter (UI) and JetBrains Mono (code), already shipped.
- No visible COSMIC/System76/Pop!_OS/GNOME/KDE branding anywhere.
- Never break boot. Commit after every completed item, boot-verify, and revert immediately on regression.
- Do not start new features (no new agent tools, no installer work) until P0 and P1 are fully evidenced.

============================================================
2. DESIGN SYSTEM (implement as ONE source of truth: crate cosmos-theme, used by uitk, shell, compositor chrome and greeter)
============================================================
2.1 Tokens (exact values; expose as Rust consts plus runtime accent):
- Spacing grid: 4px base. Allowed: 4, 8, 12, 16, 20, 24, 32, 40.
- Radii: control 8, row 6, card 12, window 12, panel/popover 14, dock 22, island fully rounded.
- Type (Inter): caption 11/14 medium (uppercase section labels, +0.4 tracking), body 13/18 regular, body-strong 13/18 semibold, title3 15/20 semibold, title2 20/26 semibold, title1 28/34 bold, large clock 64 light. Monospace 13/18 JetBrains Mono.
- Dark palette: base #14111F, window #1C1730 at alpha 0.90, sidebar #16122A at alpha 0.92, raised #262040, hairline white at 8% alpha, text primary #F4F1FA, secondary #F4F1FA at 62%, tertiary 40%.
- Light palette: base #F3F1F8, window #FBFAFE at alpha 0.92, sidebar #ECE8F5 at alpha 0.94, raised #FFFFFF, hairline black at 8% alpha, text primary #1A1625, secondary at 60%, tertiary 38%.
- Accent: extracted from the active wallpaper (dominant saturated colour, clamped to readable contrast >= 4.5:1 for text-on-accent). Fallback violet #8B5CF6. Derive accent-hover (+8% lightness), accent-pressed (-8%) and accent-tint (20% alpha) for selection.
- Semantic colours: success #34C77B, warning #F5A524, danger #F2555A, info = accent.
- Elevation: e1 (cards) 0 1 2 at 20% black; e2 (popovers/panels) 0 8 24 at 35%; e3 (windows) 0 18 48 at 45% (focused) and 0 10 28 at 30% (unfocused).
- Motion: standard 220 ms cubic-bezier(0.2,0.8,0.2,1); spring-like open = scale 0.96→1 plus fade 0→1 in 240 ms; close 160 ms. Reduce Motion = fades only, 120 ms.

2.2 Materials ("glass" that works on llvmpipe):
- On wallpaper load (and on wallpaper change), produce ONE pre-blurred copy (gaussian radius 40 at 1/4 resolution then upscaled, saturation x1.2) and share it with the shell (memfd or a cached PNG under $XDG_RUNTIME_DIR).
- Glass surface = the blurred wallpaper sampled at the surface's screen rect, plus a tint (dark rgba(28,23,48,0.55), light rgba(255,255,255,0.58)), plus 2% noise, plus a 1px inner hairline (white 10% dark / white 60% light top edge), plus elevation shadow.
- Glass is used for: menubar, dock, island, launcher/Start, Control Centre, notifications/approvals, snap flyout, switcher, context menus, greeter card.
- Windows use the translucent window colour (alpha 0.90) so the wallpaper subtly bleeds through. The compositor must honour ARGB client buffers and premultiplied alpha.
- Lite mode (Settings toggle plus auto when RAM < 2.5 GB is NOT required, keep manual): replace glass with the solid raised colour and keep the layout identical.

2.3 Iconography:
- Build an original icon set as SVG, rendered to PNG (32/48/64/128/256) at build time with resvg. No runtime SVG dependency if it costs RAM.
- App icons are superellipse "squircle" tiles (n=5) with a vivid 2-stop diagonal gradient, a subtle top highlight, a 1px inner border and a white or near-white glyph:
  Terminal (graphite #3A3F4B→#14161B, glyph >_), Files (#5AC8FA→#2F7BF6, folder), Editor (#FFC857→#F7882F, page with lines), Monitor (#5BE49B→#14A86B, pulse), Settings (#A3ACBA→#5B6472, gear), Agents (#C084FC→#EC4899, spark/star), Browser (#60A5FA→#6366F1, globe), Store/Packages (#38BDF8→#0EA5E9, bag).
- UI glyphs (toolbar, sidebar, tray): one consistent 1.5px-stroke original set at 16/20px, rounded caps, coloured only when semantic.
- File-type icons in Files: folder (blue), image, video, audio, archive, code, text, pdf. Each is distinct and coloured.

2.4 Wallpapers:
- Six original ambient gradient wallpapers (3840x2160 plus 1920x1080): Violet Dusk (default), Ocean, Coral, Aurora, Peach Lilac, Indigo Night. Make them luminous and saturated like a flagship OS default, with soft overlapping radial blobs and one bright highlight. Use dither noise to prevent banding. No muddy greys. Each has a light and dark variant (light = brighter and airier).
- Wallpaper picker in Settings with thumbnails. Switching updates accent, glass, the island and the greeter live.

2.5 Anti-slop rules (these fail review):
- No neutral grey app backgrounds, no default egui widgets, no sharp corners, no 1px grey boxes around everything, no centred-text dashboards of random stats.
- No emoji as icons, no lorem ipsum, no fake data, no "Welcome to CosmosOS!" hero banners, no purple-to-blue gradient buttons everywhere.
- Text must never clip or overlap. Respect alignment: every element snaps to the 4px grid and shares left edges.
- Every interactive element needs hover, pressed, focused (2px accent ring) and disabled states.
- Density like macOS Mail/Finder: 28px list rows, 13px body, generous but not wasteful spacing.

============================================================
3. SURFACE SPECS
============================================================
3.1 Menubar (top, 30px, glass, full width):
- Left: Cosmos logo glyph (opens a system menu: About CosmosOS, Settings, Lock, Log out, Restart, Shut down), then the focused app name in body-strong, then app menus (File, Edit, View, Window, Help) for our uitk apps via a simple menu model each app publishes over IPC. Fall back to just the app name for foreign apps.
- Centre: the dynamic island.
- Right: tray (network, volume, battery if present, Focus moon), then Control Centre button, then date and time ("Thu 8 Oct 22:14"). Workspaces move OUT of the menubar into a compact dots indicator (filled dot = current) that expands on hover. No plain digits 1-9.

3.2 Dock (left default, floating pill, glass):
- 10px from the left edge, vertically centred, radius 22, 8px padding, 48px icons with 8px gaps, magnification to 64px with smooth falloff, a small accent dot for running apps, a separator before running non-pinned apps, and a hover tooltip as a glass bubble.
- Right-click menu: Open, New Window, Keep in Dock, Quit. Drag to reorder pinned apps. Position option: left/bottom/right (Settings). Exclusive zone = pill width + 20px.
- Remove the left-edge dark strip artifact completely.

3.3 Dynamic island (top centre, original design, not a copy of Apple's):
- Idle: a 120x30 pill showing a tiny status (time-sensitive activity or nothing). Never an empty pill with a lone glyph; when idle and empty, show a subtle 36x8 handle.
- Live activities: agent running (agent icon, task name, elapsed time, Pause button), approval pending (pulse in danger/warning colour), download/copy progress, media now playing (title, play/pause), timer, screen recording, Focus on.
- Expanded (click or hover-hold 300 ms): a 420px glass card with tabs for Activities, Clipboard (last 10, click to paste), Shelf (drag files in and out). Morph animation between states.
- Agent approvals appear here FIRST, compact, with Allow / Deny.

3.4 Search or Ask / Start (Super, or the logo-click alternative = Start):
- Search mode (typing): a 680px centred glass panel with a large 20px input, results grouped (Top Hit, Apps, Files, Settings, Calculator, Commands, Ask), keyboard navigation, a preview pane for files, and `>` commands and `?` Ask (opencode/agent) with a streamed answer.
- Start mode (empty query): Pinned grid (8 per row, 56px icons with labels), Recommended (recent files and apps with real timestamps), Widgets row (clock/calendar, system load, storage, weather only if network data is real, otherwise omit), and a footer with the user avatar plus name on the left and power on the right. No overlap: compute height from content.

3.5 Control Centre (glass, 360px, top-right drop-down):
- macOS-style module grid: Wi-Fi, Bluetooth (real if BlueZ is present, else hidden, never fake), Focus, Dark Mode, Lite Mode, Screen Record. Sliders for Display brightness (if backlight exists) and Sound (wpctl). A now-playing tile when a media session exists. Each module expands to a detail view (e.g. a Wi-Fi network list via NM).

3.6 Notifications and approvals:
- Glass banners top-right under the menubar, 360px wide, app icon 32px, app name caption, title body-strong, body secondary, and actions as right-aligned buttons with centred text (primary = accent fill).
- Coalesce identical requests with a ×N badge. Show max 3 on screen, then "N more" to the Notification Centre (swipe/click the clock opens the centre with grouped history plus Clear).
- Approval card: "<agent> wants to <human verb> <object>" (e.g. "blitz-agent wants to write notes.txt in ~/Agents/blitz-agent"), a risk chip (Low/Medium/High), Deny / Allow once / Always allow, and a "Details" disclosure with formatted JSON. No stray braces. Middle-truncate paths.

3.7 Window chrome (compositor SSD, uses cosmos-theme):
- 38px titlebar fused with the app's toolbar (unified toolbar like macOS). Traffic lights 12px with 8px gaps, showing glyphs on hover. Inactive windows get grey lights and dimmed titles. The green button hover shows the snap layouts flyout (existing). Radius 12 on all corners. e3 shadow. A focused-window subtle accent hairline is optional.
- Agent-controlled windows: an accent glow border plus an agent badge on the titlebar.
- Open/close/minimise animations per motion tokens. Minimise animates toward the dock icon.

3.8 Greeter and lock screen:
- Active wallpaper (blurred), a large clock (64 light) plus date above, a centred round avatar (initials on an accent gradient if no picture), the user name, a glass password field with a submit arrow inside it, and power/keyboard buttons bottom-right. A shake animation on wrong password. Remove the stray rectangle artifacts. Native resolution.

3.9 Snap, Snap Assist, switcher, overview:
- Keep the existing logic and restyle with glass and tokens. Alt+Tab switcher = a glass strip of live thumbnails plus icons plus titles. Super+Tab or a 4-finger/hot-corner overview shows all windows plus a workspace strip (if overview already exists, restyle; if not, implement as P2).

============================================================
4. APPS (each must look native, using cosmos-theme; real data only)
============================================================
Common: a unified titlebar plus toolbar, a sidebar (where relevant) in sidebar material, 28px rows, empty states with an icon plus a one-line hint, and full keyboard shortcuts.
- Files: sidebar Favourites (Home, Desktop, Documents, Downloads, Pictures, Music) plus Locations, created on first boot via xdg-user-dirs. Toolbar: back/forward, path breadcrumbs, view toggle (list/grid), search. List view with coloured file-type icons, Name/Kind/Size/Modified, hidden files off by default (toggle with Ctrl+H). Grid view with 64px icons and image thumbnails. Real open (xdg-open), rename, new folder, delete to Trash, drag and drop. Ship a few real sample files in /etc/skel/Documents (a README.md about CosmosOS, a sample image) so screenshots aren't empty.
- Editor: tabs, line numbers, monospace, syntax highlighting for at least rs/py/js/ts/md/json/toml/sh (use syntect or tree-sitter only if the RAM budget holds; otherwise a small hand lexer), a status bar (line:col, language, UTF-8), Open/Save/Save As, unsaved dot on the tab, find (Ctrl+F). The text area fills the window: no dead block.
- Settings: macOS System Settings layout with a searchable sidebar of sections: Appearance (Light/Dark/Auto, accent incl. Auto-from-wallpaper, wallpaper picker, Lite mode, Reduce motion), Desktop & Dock (position, size, magnification), Displays (resolution, scale), Sound, Network, Bluetooth (if present), Notifications & Focus, Agents (link to the Agents app), Keyboard (shortcuts list), Users, About (CosmosOS version, kernel, memory, disk, real values). Every control works and persists to ~/.config/cosmos/config.json.
- Monitor: CPU/RAM/swap/disk/network live graphs (anti-aliased area charts in accent), a process table sortable by CPU/RAM with Kill (confirmation).
- Terminal: dark and light palettes from the tokens, 13px JetBrains Mono, tabs, padding 12, translucent background option.
- Agents: sidebar of agents with status dots; detail panes Overview (current task, live log), Permissions (files/network/tools as toggles), Approvals history, Audit log (table), Rollback (btrfs snapshot timeline with Restore that really calls snapper; if snapper isn't configured, configure it in the image). No empty "read:" fields.

============================================================
5. EXECUTION ORDER AND TIME BOXING
============================================================
Work in tiers. Do not start a tier until the previous one is merged, boot-verified and screenshotted.

P0 — THE LOOK (must ship):
 1. Remove the left strip artifact.
 2. cosmos-theme crate plus a uitk Visuals override, so ALL apps get tokens, the translucent window fill, rounding and spacing. Compositor ARGB/alpha correctness.
 3. Pre-blurred wallpaper glass on menubar, dock, launcher/Start, Control Centre, notifications.
 4. Squircle app icon set plus file-type icons, used everywhere.
 5. Six luminous wallpapers plus accent extraction plus wallpaper picker.
 6. Light mode fully propagated (shell, chrome, all apps, terminal palette).

P1 — THE SURFACES:
 7. Menubar spec (system menu, app name, workspace dots, tray, clock format).
 8. Start/Search spec incl. no overlap and real Recommended.
 9. Notifications plus approvals spec plus Notification Centre.
10. Island states (idle handle, agent activity, approval, clipboard/shelf expanded).
11. Files plus Editor plus Settings to spec (sample files in skel).
12. Greeter/lock to spec.

P2 — POLISH:
13. Control Centre module detail views. 14. Window animations incl. minimise-to-dock. 15. Monitor graphs. 16. Agents app panes. 17. App menus in the menubar. 18. Overview/switcher restyle. 19. Dock context menu plus reorder.

If time runs short, stop at a tier boundary with everything merged and evidenced. A finished P0+P1 beats a half-done P2.

============================================================
6. VERIFICATION GATES (mandatory per item)
============================================================
For every item:
- cargo build --release for the workspace with zero warnings in the touched crates, and cargo clippy with no new warnings.
- Rebuild the image, boot in QEMU at 1920x1080 (and once at 1366x768 at the end), and run the existing drive harness. Zero panics in the journal (journalctl -b | grep -i panic must be empty).
- Capture a real QEMU screendump of the item and look at it yourself at 100% zoom. Check for: clipped text, overlap, misalignment, grey-default widgets, artifacts, and contrast < 4.5:1. If any fail, fix before moving on.
- Commit with a clear message; update docs/STATUS.md (append, max 3 lines per item).

At the end:
- Run measure tooling and append to docs/budgets.md: idle RAM (2 GB VM, after 60 s idle, `free -m` plus `smem`/PSS of shell, compositor and apps), image size, install size, boot-to-desktop time. All must be within the budgets in section 1.
- 30-minute soak with the harness cycling apps/snap/launcher: no panics, RSS growth < 10%.

============================================================
7. FINAL EVIDENCE (docs/evidence/v3-*.png, all real 1920x1080 screendumps)
============================================================
 v3-01-desktop-dark.png (Violet Dusk, empty desktop: menubar, island, dock)
 v3-02-desktop-light.png (same, light)
 v3-03-windows.png (Files grid view with real files plus Editor with a highlighted .rs file, snapped side by side)
 v3-04-start.png (Start mode)
 v3-05-search.png (typing "set": grouped results)
 v3-06-control-centre.png
 v3-07-notifications.png (one coalesced approval plus a normal notification)
 v3-08-island-expanded.png (clipboard/shelf)
 v3-09-island-agent.png (agent live activity)
 v3-10-settings-appearance.png (wallpaper picker plus accent)
 v3-11-ocean.png (desktop on Ocean: accent changed)
 v3-12-greeter.png
 v3-13-snap-flyout.png
 v3-14-monitor.png
 v3-15-agents.png
 v3-16-lite-mode.png (glass off, layout identical)
 v3-17-1366x768.png (layout holds on a small screen)
Also commit docs/evidence/v3-REPORT.md: a table of every item in sections 3-5 with Done/Partial/Not started, the commit SHA and the screenshot name. Be honest: mark anything incomplete as Partial with the reason. Never claim Done without a screenshot.

============================================================
8. SELF-REVIEW BEFORE YOU SAY YOU'RE DONE
============================================================
Put v3-01, v3-03 and v3-04 next to your memory of macOS Sequoia/Tahoe and Windows 11 at the same scale. For each, ask: would a design-literate person believe this is a shipping commercial OS? If no, list what gives it away, fix the top 3, and re-shoot. Then grep the touched code for TODO, FIXME, unimplemented!, todo!, placeholder, mock, dummy, lorem: the count must be zero in the code you added.

Deliver: merged PR(s) to main, the screenshots, budgets.md updated and v3-REPORT.md. Start now with P0 item 1.
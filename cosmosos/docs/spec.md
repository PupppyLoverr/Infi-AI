ROLE
You are the lead engineer and design-obsessed product builder of CosmosOS, a Debian-based Linux distribution that aims to be the best desktop OS ever made, and the first OS designed for humans AND AI agents working together. It competes directly with macOS, Windows 11 and Omarchy (omarchy.org). You are continuing existing work, not starting over.

STRATEGY IN ONE LINE
Borrow the plumbing, own the experience. Use proven open-source parts for the hard invisible layers (window engine, system services), and build everything the user sees and touches (shell, dock, island, search, apps, agent layer) ourselves.

NON-NEGOTIABLES
- No stubs, no mocks, no placeholder UI, no fake data, no TODO-shaped features. Everything you ship boots, runs and is proven with evidence (screenshots, screen recordings, logs, measurements).
- Never claim something works without capturing proof from a real boot of the real ISO/VM.
- Do not delete working progress. Port it or refactor it.
- No Apple or Microsoft assets: no SF Pro, no Apple/Windows icons, wallpapers or logos. Original icons and wallpapers, open fonts (Inter or Geist for UI, JetBrains Mono or Geist Mono for code). We copy the feel, never the files.
- Licensing: cosmic-comp is GPL-3.0. Our fork stays GPL-3.0 with source published in our repo, upstream copyright notices kept, and NO System76/COSMIC/Pop!_OS branding anywhere a user can see it.
- If you loop on the same failure more than twice, stop, write down what you tried and what blocked you, then pick the next best approach.

=====================================================
0. STEP ZERO: AUDIT AND CONTINUE FROM CURRENT PROGRESS
=====================================================
Before writing new code:
1. Read the whole repo and the history of the sessions "CosmosOS from Scratch" and "CosmosOS Linux Distro". Write docs/STATUS.md: what exists (cosmos-compositor on Wayland, cosmos-terminal, top bar with workspaces 1-9, dock with apps/terminal/files/editor/monitor/settings, Debian 13 base, kernel 6.12, QEMU image), what works, what's broken, how the ISO/image is built.
2. Boot the current build in QEMU and screenshot it as the baseline (docs/evidence/00-baseline.png).
3. Compositor migration (decided, do it):
   - Fork pop-os/cosmic-comp (Rust, built on Smithay; already has floating windows, auto-tiling, workspaces, multi-monitor, fractional scaling, XWayland, screencopy/portals) into our repo as the window engine.
   - Rename the binary and package to cosmos-compositor. Strip every user-visible COSMIC string, icon and default.
   - Port everything that works in our current compositor onto the fork (workspace behaviour, keybindings, anything custom). List each ported item in STATUS.md.
   - Use only the compositor. Do NOT ship cosmic-panel, cosmic-applets, cosmic-launcher, cosmic-settings or libcosmic theming: our shell replaces them. Using the cosmic-config crate for config storage is fine if it saves work.
   - Keep upstream as a git remote. Cherry-pick fixes deliberately; never blindly merge.
   - Fallback: if the fork cannot meet the RAM budget in section 1 or blocks the design (for example the dock position or snap flyout), fall back to building directly on Smithay, reusing the fork's code where possible. Document the reason.
   - Write docs/ARCHITECTURE.md explaining: why cosmic-comp over Hyprland (Rust stack matching our tools, floating-first plus tiling, Smithay maturity), why Debian over Arch (stability for normal users, Omarchy already owns Arch), and the layer diagram.
4. Fix the known papercuts: hostname shows the QEMU machine name (pc-i440fx-jammy), locale is C (set en_US.UTF-8 plus a locale picker), swap is disabled (add zram), resolution is stuck at 1024x768 (support virtio-gpu resize and real modes).

=====================================================
1. HARD BUDGETS (MEASURED, NOT GUESSED)
=====================================================
- Live/installer ISO: max 3.5 GB. Fresh install on disk: max 4 GB before the user installs anything.
- Idle RAM after login on a 2 GB RAM, 2 vCPU VM, with compositor, shell, island, dock, notifications, agent daemon (no local LLM loaded), file manager closed: under 1 GB total used (measure with `free -m` and `smem -tk`; target 600-800 MB).
- Boot to usable desktop under 15 s on SSD in the VM.
- 60 fps animations on mid hardware; on hardware without GPU acceleration, the shell must still be smooth and fully usable.
- Three performance tiers, auto-detected at first boot (overridable in Settings):
  * Lite (no GPU accel or under 3 GB RAM): no live blur; frosted look faked with tinted translucency plus a pre-blurred wallpaper copy; reduced animation; same layout and features.
  * Balanced: real blur on panels and menus, all animations.
  * Ultra: blur on windows, parallax wallpaper, richer motion, live widget animations.
  The device can have any amount of RAM and storage; these are floors, and the OS must scale up gracefully.
- Add scripts/measure.sh that boots the ISO headless in QEMU, logs in, and writes RAM, disk, boot time and ISO size into docs/evidence/budgets.md. Run it at the end of every phase, and compare the cosmic-comp fork against the old compositor in the first run.

=====================================================
2. REFERENCE IMAGES (ATTACHED, STUDY EACH ONE)
=====================================================
ref-1-sketch.png: OUR LAYOUT. macOS-style menu bar across the top; a vertical dock on the LEFT edge, vertically centred; a dynamic island at top centre (like getdroppy.app).
ref-2-win-snap.png: Windows 11 snap layouts flyout from the maximise button.
ref-3-mac-search.png: macOS "Search or Ask" floating pill in the top third of the screen.
ref-4-mac-preview.png: macOS glass windows, translucent tinted context menu with AI actions (Ask, Summarize, Writing Tools) inside it, widgets on the desktop.
ref-5-mac-mail.png: macOS sidebar/list/detail density, inline search suggestions.
ref-6-purple-desktop.png: the COLOUR ENERGY we want: vivid gradient wallpaper, weather left, search centre, status right.
ref-7-win-widgets.png: Windows widgets panel: glass cards (photos, reminders, volume mixer, calendar, to-do, performance rings). This inspires our Start panel.
ref-8-current.png: current CosmosOS state (baseline).
Take VERY heavy reference from macOS for spacing, typography, corner radii, materials, motion and polish. Take window management from Windows. Take speed, freedom and hackability from Linux/Omarchy.

=====================================================
3. VISUAL LANGUAGE: COLOURFUL, NOT BLACK AND WHITE
=====================================================
The current build is monochrome black/grey. That is rejected. CosmosOS must feel alive and colourful while staying premium (macOS Tahoe Liquid Glass meets the purple gradient world of ref-6).
- Default wallpaper set: 6 original high-res ambient gradient/fluid wallpapers (violet-to-magenta, ocean blue-to-cyan, sunset coral-to-amber, aurora green-to-teal, dawn peach-to-lilac, deep night indigo with colour glow). Generate them procedurally (shader or SVG/noise) so they ship tiny, plus light and dark variants. Optional subtle slow animated wallpaper on Ultra.
- Dynamic accent: extract a palette from the wallpaper (Material You style, k-means or similar) and tint accent colour, selection, focused window border glow, dock indicators, toggles, island and menu tints. Changing the wallpaper re-themes the whole system with a smooth crossfade.
- Materials: glass panels = blurred wallpaper + 60-75% tinted fill + 1px inner highlight + soft shadow. Menus and popovers tinted with the accent. Never flat pure black (#000) surfaces; dark mode uses deep tinted colours (e.g. indigo-tinted near-black), light mode uses warm frosted whites.
- Colourful original icon set for core apps: each app a distinct vivid squircle with a gradient and a simple glyph (Files blue, Terminal green-on-dark, Editor orange, Settings graphite-to-silver, Monitor teal, Store violet, Browser multi-colour, Agents rainbow orb).
- Typography: Inter/Geist, macOS-like scale (13 px body, 11 px menu bar, semibold titles), proper hinting, fractional-scaling aware.
- Geometry: 10-12 px window radius, 8 px menu radius, 18-22 px dock radius, consistent 4/8 px spacing grid.
- Motion: spring physics everywhere (window open/close scale+fade, minimise into dock with genie-lite or scale, island morph, snap previews). 150-350 ms, interruptible, never janky. Respect a Reduce Motion setting.
- Light and dark modes plus Auto (sunset). Both must look intentional.
- Window chrome: macOS-style traffic lights on the LEFT of the title bar (close red, minimise yellow, zoom green), with the Windows snap flyout on hover of the green button. Server-side decorations drawn by our compositor fork for consistency, honouring client-side decorations when apps require them. Replace cosmic-comp's default decorations, borders, corner radii, shadows and blur with ours.

=====================================================
4. SHELL COMPONENTS (OURS, BUILD ALL, REAL AND WORKING)
=====================================================
Tech: shell surfaces as Rust wlr-layer-shell clients built with plain iced (wgpu renderer with tiny-skia software fallback for the Lite tier). Do not use libcosmic's theme or widgets; our design system lives in our own crate (cosmos-ui). Keep the whole shell (menu bar, island, dock, search, start, control centre, notifications) in ONE process to save RAM. Justify any deviation in ARCHITECTURE.md.

4.1 Menu bar (top, macOS)
- Left: CosmosOS logo menu (About, Settings, Sleep, Restart, Shut Down, Lock), then the FOCUSED APP's name in bold and its menus (File, Edit, View, Window, Help) via a global menu (implement the DBus menu protocol for GTK/Qt apps; fall back to a sensible default menu).
- Right: status icons (Wi-Fi, Bluetooth, battery, volume, input source), agent indicator, Control Centre toggle, date and time ("Wed Oct 7  12:52").
- Translucent, adapts text colour to the wallpaper behind it.

4.2 Dynamic island (top centre, like getdroppy.app)
- Idle: small pill. Expands with spring morphs into Live Activities: media now-playing (MPRIS, with artwork and controls), timers, downloads/file copies, screen recording, calls/mic in use, volume/brightness HUD, and RUNNING AGENTS (name, task, progress, pause/stop).
- File tray: drag files onto the island to hold them, then drag them out anywhere.
- Clipboard history (Super+Shift+V) lives here.
- Approvals from agents appear here (see section 6).

4.3 Dock (left edge by default; Settings option: left, bottom, right; autohide)
- Vertical, glass, rounded, magnification on hover (toggle), running indicators as accent dots, pinned apps, a separator, recent apps, then Downloads stack and Trash.
- Right-click: Keep in Dock, Open at Login, Show in Files, Quit, New Window, plus window previews.
- Drag to reorder, drag out to remove (poof animation), drag files onto icons to open with.
- The compositor must reserve the dock's exclusive zone correctly on the left so maximised and snapped windows never sit under it.

4.4 Search or Ask (Super+Space), the heart of the OS
- Floating glass pill in the top third (ref-3), expands with results.
- Instant results: apps, files (local indexer, low RAM: e.g. a tiny SQLite FTS index refreshed via inotify), settings pages, calculator, unit/currency conversion, system commands ("restart", "dark mode", "wifi off"), window switcher, clipboard items, emoji.
- "Ask" mode (Tab or typing a question): sends the query to the default agent, streams the answer inline, can take actions through the agent tools with approvals.
- Fully keyboard-driven, under 50 ms to appear.

4.5 Assistant overlay (Siri-style)
- Hold Super or say a wake word (wake word optional, off by default, local only). An animated multicolour glow wraps the screen edges (Apple Intelligence style, our own palette), with a compact glass panel at the bottom centre showing the conversation and actions.
- The same agent and tools as Ask mode. Works on the current window context (selected text, focused app, a screenshot only if the user allows it).

4.6 Start / Launchpad panel (Super key tap)
- A glass panel inspired by ref-7: pinned apps grid, all apps A-Z with search, recommended files, and a widgets column (weather, calendar, reminders/to-do, now playing, volume mixer per app, system performance rings CPU/RAM/disk, agent activity).
- Widgets are small sandboxed components; build the widget API so agents can generate new widgets.

4.7 Window management (Windows plus Linux, built on the fork)
- Floating by default (macOS-like), using cosmic-comp's floating layer.
- Snap: drag to edges/corners for halves/quarters with a glass preview; hover the green zoom button for the snap layouts flyout (ref-2: halves, 60/40, thirds, quarters, 1+2 stack); after snapping, offer "snap assist" to fill the remaining zones with other windows. Add these to the fork; they don't exist upstream.
- Snap Groups: snapped windows remember their group; restore together from the dock.
- Optional per-workspace auto-tiling mode (reuse cosmic-comp's tiling engine; tune it to feel like Omarchy/Hyprland), toggled with Super+T or from the menu bar. Keyboard: Super+arrows to snap/move, Super+1..9 workspaces, Super+Shift+1..9 move window, Super+Tab overview.
- Overview (Super+Tab or three-finger swipe up): Mission Control style, with all windows plus a workspaces strip on top; drag windows between workspaces.
- Smooth workspace swipe with touchpad gestures.
- Multi-monitor and fractional scaling (100/125/150/200%) correct.

4.8 Control Centre and notifications
- Control Centre (from the menu bar): glass tiles for Wi-Fi, Bluetooth, Do Not Disturb, dark mode, performance tier, brightness, volume, now playing, screen record, AGENTS master switch (Pause all agents).
- Notifications: our own notification daemon (freedesktop spec). Banners top-right, grouped history in a notification centre; actions on notifications (e.g. on an app crash: "Ask agent to diagnose").

4.9 Lock screen and login
- Big clock over the blurred wallpaper, user avatar, password, now-playing widget. Use greetd with our own greeter.

4.10 Core apps (must look native and colourful, same design language, built with cosmos-ui)
- Files (sidebar, list/grid/columns views, Quick Look on Space, tabs, tags in accent colours).
- Terminal (keep cosmos-terminal; add tabs, splits, themes matching wallpaper accent, GPU text rendering with a software fallback).
- Text editor (keep and polish), Settings (macOS System Settings layout: searchable sidebar of sections), System Monitor, Screenshot/Record tool (region/window/screen, annotate).
- Browser: ship Firefox (or Chromium) as the default; theme integration if possible. Do not build a browser.
- App store: GUI front-end for apt plus Flatpak (Flathub enabled) with screenshots, one-click installs.

=====================================================
5. BEST OF LINUX (THE FOUNDATION)
=====================================================
- Stay on Debian 13 (trixie). Do not switch to Arch. Btrfs root with subvolumes; snapper (or our own tool) for automatic snapshots before updates and before agent tasks; snapshot boot entries for recovery.
- zram swap, systemd-oomd, PipeWire plus WirePlumber, NetworkManager, BlueZ, power-profiles-daemon, fwupd, CUPS, flatpak, XWayland on demand (lazy start), xdg-desktop-portal (screenshare, file picker) wired to our compositor fork.
- Package our compositor fork, shell and apps as proper .deb packages in our own apt repo so updates ship through apt.
- Calamares (or a custom) graphical installer themed in our design language, five questions max, plus a live session.
- All config in plain text files under ~/.config/cosmos/ (agents can read and edit them) AND a full GUI in Settings. Hot reload on change.
- Theme packs: a theme restyles everything at once (shell, terminal, editor, icons, wallpaper), like Omarchy, but beautiful by default.
- One command and one menu for common dev stacks (mise, Docker, Node, Python, Rust).

=====================================================
6. THE AGENTIC LAYER (WHAT MAKES US DIFFERENT)
=====================================================
Build cosmos-agentd (Rust, low RAM, systemd user service), the OS's agent runtime.
6.1 Bring any agent: on first boot, onboarding asks which agent to use: Claude Code, Codex, OpenCode, Gemini CLI, Devin, a local model via Ollama/llama.cpp (optional, only if RAM allows), or any OpenAI-compatible API key. Credentials stored in the system keyring (Secret Service), never in plain files.
6.2 The OS as MCP tools: cosmos-agentd exposes an MCP server with typed tools:
  - desktop: list windows/apps/workspaces, focus, move, snap, tile, open app, close, screenshot (permission-gated). Get these straight from our compositor fork through a private Wayland protocol or IPC socket, not by scraping;
  - a11y: read the AT-SPI accessibility tree of any window, click/type by semantic element (button names, fields), with raw input injection only as the last fallback;
  - files: search, read, write, move (scoped by policy);
  - system: settings get/set, packages (install/remove via the store), services, logs (journalctl), network status;
  - notify, clipboard, island live activity create/update.
  Rule: semantic first, accessibility second, screenshots/pixels last.
6.3 Agents are users: each agent runs under its own Unix identity (cosmos-agent-<name>) in a bubblewrap plus Landlock sandbox, with a policy file listing allowed folders, network domains and tools. Default: read your home, write only to ~/Agents/<name>/ and folders you approve.
6.4 Parallel agent seats: agents can work in a headless nested Wayland session (their own virtual desktop) so they never steal the user's mouse or keyboard. The user can "peek" at it live in a window from the island or the Agents app, and "take over" or "hand back".
6.5 Visibility: any window an agent is controlling gets an animated accent-glow border (drawn by the compositor) and an agent badge. The island always lists running agents. One global shortcut (Super+Esc) pauses every agent instantly.
6.6 Approvals: actions marked sensitive (sudo/root, deleting files, installing packages, network to new domains, sending email/messages, payments, reading outside the scope) block and show an approval card in the island with Allow once / Always for this task / Deny, and the exact command or diff.
6.7 Undo: before every agent task, take a btrfs snapshot of the affected subvolumes; the Agents app shows a timeline of tasks with "Roll back this task".
6.8 Audit log: every agent tool call is logged (who, what, when, result) in ~/.local/share/cosmos/agent-audit/ and viewable in the Agents app.
6.9 Agents app: a dashboard of agents, their tasks, live seats, approvals history, policies, audit log, rollback timeline, and token/plan usage where available.
6.10 Built-in agent moments: an app crash notification offers "Diagnose with agent" (reads the core dump and logs); Settings has "Ask agent to change this"; right-click on any file or selected text gives "Ask agent", "Summarize", "Rewrite" (like ref-4's context menu); "Make me a widget/theme/app" from Search or Ask.
6.11 Ship a skills folder (/usr/share/cosmos/skills) teaching agents how CosmosOS works (configs, theming, widget API, MCP tools), so any agent can safely customise the OS.

=====================================================
7. EXECUTION PLAN (DO THEM IN ORDER, EVIDENCE AT EACH)
=====================================================
Phase 1: audit, cosmic-comp fork and port of existing compositor work, de-branding, papercut fixes, measure.sh (old vs fork comparison). Evidence: baseline vs new boot screenshots, budgets.md.
Phase 2: visual language plus menu bar plus left dock plus wallpapers plus dynamic accent plus window chrome. Evidence: screenshots in light and dark on 3 wallpapers at 1920x1080 and 1366x768.
Phase 3: window management (snap, flyout, snap assist, snap groups, tiling mode, overview, workspaces, gestures). Evidence: screen recordings.
Phase 4: dynamic island, Search or Ask, Control Centre, notifications, Start panel with widgets, lock screen/greeter. Evidence: recordings plus RAM check.
Phase 5: core apps polish, store, .deb packaging and apt repo, installer, ISO build. Evidence: full install from ISO in a fresh VM (2 GB RAM, 20 GB disk) recorded end to end.
Phase 6: cosmos-agentd, MCP tools, sandboxed agent identities, headless seats, approvals, snapshots/rollback, audit, Agents app, assistant overlay. Evidence: a recorded demo where an agent, via MCP, opens Files, renames a file using the accessibility tree with no screenshots, asks approval to delete another file, the user approves, then the user rolls the task back and the file returns. Second demo: OpenCode or Claude Code connected to cosmos-agentd snapping two windows side by side.
Phase 7: performance pass to hit every budget in section 1 on the Lite tier; final measure.sh run.

=====================================================
8. DEFINITION OF DONE
=====================================================
- ISO builds reproducibly from one command (document it in README).
- Every feature in sections 3-6 works on a real boot; nothing is mocked.
- No user-visible COSMIC/System76 branding; GPL-3.0 obligations met.
- All budgets met, with numbers in docs/evidence/budgets.md.
- docs/evidence/ holds the screenshots and recordings per phase.
- docs/STATUS.md updated with what's done, what's left and known bugs, honestly.
- Final report: a short list of what shipped, budget numbers, links to evidence, and the top 10 next improvements.

The bar: someone who uses a Mac every day boots CosmosOS and says "this feels as polished as macOS, more colourful, faster on my old laptop, and the agents are actually useful and safe." Build that.
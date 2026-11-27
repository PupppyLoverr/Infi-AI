# Handoff

**Now:** Solyra runs on Hyprland 0.57.2 with the Quickshell shell, verified in
the CI VM and in the live ISO. Each push publishes screenshots and logs to
`docs/shots/vm/` (and `docs/shots/iso/`).

## State
- Status per item: PLAN.md §5 (✅ / 🟡 / ⏳) and §10.
- **Terminal** (`apps/terminal`, binary `solyra-term`):
  - zgx's own terminal app (MIT, examples/terminal at `3218e80e`), ported to
    Linux;
  - Luau UI in `ui/`, compiled in by `build.rs`;
  - `ZGX_SCRIPTS=source` loads `ui/` live from a checkout;
  - its own Cargo workspace;
  - the older terminal stays as `solyra-term-classic` in `apps/`.
- **Finder** (`apps/solyra-files`): clear Liquid Glass, floating and centred.
- **Shell** (`shell/qs`):
  - Dynamic Island (`Island.qml`): idle pill, live activities with a split
    bubble, HUD, Spotlight search (`SearchModel.qml`, `SearchView.qml`),
    expanded tabs;
  - dock, menu bar, Control Center (Night Shift, recording, per-app volume);
  - hot corners, desktop widgets, notifications, snap layouts, Mission
    Control, lock.
- **Compositor** (`shell/hypr/hyprland.lua`, Lua config):
  - acrylic refraction blur;
  - macOS floating windows unless auto-arrange (read from `look.json`);
  - drag-to-edge snapping;
  - spring animations.
- **ISO:** 1745 MiB, boots into the Solyra desktop.
- **CI:** warm caches for cargo (registry, git, both target dirs) with a
  prefix fallback; packages reuse the built binaries.

## Next
1. Read the newest VM shots:
   - the island (07c live activities, 07d recording, 08 search in island);
   - the new terminal (12);
   - the Finder (15b);
   - the desktop widgets (01).
2. RAM is over budget (765 MB in the llvmpipe VM); measure on a real GPU.
3. Not built:
   - DBusMenu app menus;
   - Stage Manager;
   - voice transcription (whisper is AUR-only).

## How to run
- VM test: push to the paths in `.github/workflows/vm.yml` (shell, tools/vm,
  apps). Shots land in `docs/shots/vm/`.
- ISO: `solyra-iso` workflow.
- Terminal locally:

  ```bash
  cd apps/terminal && cargo run --release
  ```
- Finder and classic terminal:

  ```bash
  cd apps && cargo build --release -p solyra-files -p solyra-term-classic
  ```

## Notes
- Quickshell stops at the first QML error: read `log-qs.log` first.
- The new zgx renders black under this sandbox's headless weston (pixman);
  judge it in the VM.

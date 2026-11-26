# Handoff

**Now:** the shell runs on Hyprland 0.57.2 in the CI VM. We are iterating
from the screenshots and logs the VM publishes to `docs/shots/vm/` (and
`docs/shots/iso/` for the ISO).

## State
- Status per item: PLAN.md §5 (✅ / 🟡 / ⏳) and §10.
- Terminal (`apps/solyra-term`):
  - one app: recording A plus the Claude chat inside the glass;
  - zeron icons;
  - shell integration (OSC 7/133);
  - settings popover (themes, glass, font).
- Finder (`apps/solyra-files`): macOS 27 layout, clear frosted glass.
- Shell (`shell/qs`, QML) and `shell/hypr/hyprland.lua` (Hyprland 0.57 is
  configured in Lua; `.conf` is ignored). Dispatchers are Lua expressions,
  e.g. `hl.dsp.window.close()`.
- Glass: `shell/qs/Glass.qml` (Liquid Glass material) over Hyprland blur
  (layer rules with ignore_alpha).
- ISO (`iso/`): 1745 MiB, boots to Hyprland live (greetd logs in solyra).
- Packages: `pkg/*/PKGBUILD`, built by CI with makepkg.

## Next
1. Read the newest VM shots: the dock (fixed binding), the Control Center
   layout, Liquid Glass, and idle RAM after the trims. Fix what they show.
2. Read the ISO shots with the shell; fix the live session if needed.
3. Open items:
   - backdrop refraction plugin;
   - DBusMenu global menu;
   - snap zones while dragging;
   - RAM over budget (needs a real-GPU measurement);
   - suggested extras (need the user's approval).

## How to run
- VM test: push to `shell/**`, `tools/vm/**` or `apps/solyra-term/**`
  (workflow `solyra-vm`). Shots land in `docs/shots/vm/`.
- ISO: workflow `solyra-iso` (also triggered by `shell/**`).
- Local: `cargo build -p solyra-term -p solyra-files` in `apps/`.
- Shell on any Hyprland: copy `shell/qs` to `~/.config/quickshell` and
  `shell/hypr/hyprland.lua` to `~/.config/hypr/`.

## Notes
- Quickshell stops at the first QML error: read `log-qs.log` first.
- llvmpipe inflates RAM in CI.

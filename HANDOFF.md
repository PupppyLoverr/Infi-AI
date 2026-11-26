# Handoff

**Now:** building the Hyprland + Quickshell shell and the one-app terminal.
The tests run in the `solyra-vm` workflow: an Arch VM on CI that screenshots
every surface and measures idle RAM, disk and boot. Decisions are in PLAN.md
§8, and tonight's requirements in PLAN.md §9.

## Done
- M1: Plasma measured at 1144–1192 MB. Decision: Hyprland + Quickshell on
  Arch, ≤250 MB idle budget.
- Terminal (`apps/solyra-term`, zgx): one app matching both recordings.
  - Recording A: titlebar tab pills with per-pane glyphs, `+` menu
    (Terminal / Claude Code / Codex), split buttons, rounded pane cards and
    the Agents sidebar.
  - Recording B: the Claude Code chat view (prompt, bubbles, Thinking /
    Writing Ns, composer chips for mode, model and effort, send/stop, cost)
    on a fixed session id, and `>_` swapping to `claude --resume <id>`.
  - Verified live on our compositor (screenshots termA/termB). 16 tests
    pass.
- Shell (`shell/qs`, QML) and `shell/hypr/hyprland.conf`:
  - menu bar with menus;
  - dock with magnification, bounce, running dots, background-app hollow
    dot and "Stop Running in Background";
  - notch: idle pill, live activity, HUDs (volume, brightness, battery,
    caps), and the expanded media, shelf, clipboard and tools tabs (High
    Alert, Termi-Notch);
  - Spotlight "Search or Ask" (apps, files, calculator, actions; Ask via
    the claude/codex CLIs);
  - Control Center (Wi-Fi, Bluetooth, Focus, auto-arrange, display, sound
    and glass sliders, now playing, accessibility);
  - notifications server, Windows 11-style snap layouts, auto-arrange;
  - Droppy-style lock screen: ambient art, clock, notch lock animation,
    lrclib synced lyrics, media, PAM.

## Next
1. Read the `solyra-vm` artifacts (shots/*.png, report.txt, log-qs.log).
   Fix QML/Hyprland errors from log-qs.log and log-hypr.log, then re-push.
2. M4 glass plugin (Hyprland, GPL-3.0) porting `comp/` refraction.
3. Droppy items still open: Notchface (camera), Bluetooth headphones HUD,
   reminders/calendar glance, OCR, voice transcribe, element capture,
   tracked folders, multi-display settings.
4. M8 archiso.

## How to run
- Terminal locally: `cargo build -p solyra-term` in `apps/`, then run
  `SOLYRA_TERM_LAYOUT=split SOLYRA_TERM_SIDEBAR=1` or `SOLYRA_TERM_CHAT=1`.
- Shell: `quickshell -p shell/qs` inside Hyprland; IPC with
  `qs ipc call shell <spotlight|query|control|lock|snap|island|hud|glass|arrange>`.
- VM: push to `shell/**`, `tools/vm/**` or `apps/solyra-term/**`, or
  dispatch `solyra-vm`.

## Open problems
- This container cannot run Arch or Hyprland, so shell verification happens
  only in CI.
- llvmpipe in CI inflates RAM.
- The global menu shows standard menus wired to real actions; full DBusMenu
  export from apps is not done yet.
- The hyprexpo plugin (Mission Control) is not installed in the VM yet.

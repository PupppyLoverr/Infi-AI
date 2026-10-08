# Port map — current compositor → cosmos-compositor (cosmic-comp fork)

Everything our compositor does today that must exist in the fork before we
can call it a port. The shell and apps are Wayland clients (layer-shell +
xdg-shell + our shm canvases) — they don't change, only the window engine
beneath them and the IPC bridge that connects them to it.

## A. IPC surface (cosmos-ipc) — retarget the socket onto the fork

Requests to support: `Ping`, `Subscribe`, `ListWindows`, `ListWorkspaces`,
`FocusWindow`, `MoveWindowToWorkspace`, `CloseWindow`, `SwitchWorkspace`,
`ToggleLauncher`, `ToggleHelp`, `SnapAssistPick`, `SnapAssistDismiss`,
`SetConfig`, `GetConfig`, `QuitSession`.

Events: `Pong`, `Windows`, `Workspaces`, `Config`, `LauncherToggled`,
`HelpToggled`, `Switcher`, `SnapAssist`, `SessionEnding`, `Error`.

The fork has no such socket — add a `cosmos-ipc` thread inside it (unix
socket at `/tmp/cosmos-ipc-$UID.sock`, newline-delimited serde_json) so the
shell code is untouched.

## B. Keybinds (input_handler.rs KeyAction set)

Super+Space launcher, Super+? help, Alt+Tab/Alt+` switcher, Super+T tiling,
Super+D show-desktop, Super+E files, Super+Return terminal, Super+Q close,
Super+{1-9} workspaces, Super+Shift+{1-9} move-to-ws, Super+{Left,Right}
snap, Super+Up maximize/restore, Ctrl+Alt+T terminal, media keys
(wpctl volume + brightnessctl), VT switch passthrough.
cosmic-comp binds via RON config — map ours on top; keep ours consistent.

## C. Window management semantics

- Snap halves/quarters via drag-edge preview + keys; Snap Assist picker on
  half-snap (compositor emits `SnapAssist` event, shell draws the card,
  `SnapAssistPick` commits it).
- Snap groups: windows snapped together restore/raise together.
- Master+stack tiling toggle per workspace (Super+T).
- Minimize + show-desktop (Super+D) with restore order.
- Always-clamped placement inside the output zone (our refit loop fix).
- Focus-follows-flash on raise; activation-token-equivalent for launches.

cosmic-comp has floating/tiling/workspaces natively; snap preview + assist +
groups + minimize are ours to add.

## D. SSD chrome (our look, not COSMIC's)

Traffic-light discs L→R (red close / yellow minimize / green zoom-flyout),
hover glyph reveal, centred title, 12px top radii, unfocused greying.
Hover on green = snap-layout flyout (the Win11 gesture from the spec).
cosmic-comp's builtin decorations are off by default for xdg apps — we add
ours (its `cosmic-workspace` decorations must stay disabled).

## E. Motion system

Open/close scale+fade, workspace slide, launcher/flyout slide-in, minimize
genie-lite. cosmic-comp has minimal animation; port `anim.rs` concepts into
its render loop (per-element transform/alpha interpolation + damage).

## F. Rendering/perf work to keep

- Decal texture cache keyed on content hash (PR #67/#83 class of fix) —
  audit whether the fork re-uploads textures per frame on llvmpipe.
- Software-GL correctness on llvmpipe/TCG (the whole sweep-defect saga):
  release-gated buffer handling, no recycle-while-displayed, opaque clears.
- Damage tracking that survives scroll/partial updates.

## G. Layer surfaces / shell integration

Shell already uses layer-shell: menubar (top), dock (left edge per new
layout — anchor change is shell-side), launcher/flyout/notifications/
assist/cheatsheet (overlay layer), wallpaper (background). The fork must
honor exclusive zones + input regions the same way; verify bbox-vs-region
pointer routing behavior (our #50/#51 fixes were smithay `layer_under` —
check if the fork hits the same path).

## H. Theming/config

Accent presets (azure/violet/forest/ember/rose/mono), dark/light mode —
`SetConfig`/`GetConfig` on IPC today; in the fork it can be a cosmos config
section, but keep the IPC events so shell widgets work unchanged.

## I. Explicitly NOT ported / disabled

cosmic-panel, cosmic-applets, cosmic-launcher, cosmic-settings, libcosmic
theming, all COSMIC/Pop!_OS/System76 user-visible strings and assets. Keep
the GPL-3.0 license + upstream copyright; `upstream` remote for deliberate
cherry-picks only.

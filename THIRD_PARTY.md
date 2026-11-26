# Third-party notices

## Solar icons (UI glyphs)

`shell/panel/assets/glyphs/*.png` are baked from **Solar** (Linear style) by
[480 Design](https://www.figma.com/community/file/1166831539721848736),
licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
Changes: rasterised to 48 px alpha masks and tinted at draw time.
`web/src/solar.ts` carries the same icons as SVG path data (unchanged
shapes, drawn in `currentColor`).

## solyra-web (Tauri + React)

`web/` bundles [React](https://react.dev) (MIT), [xterm.js](https://xtermjs.org)
and its fit add-on (MIT), and is built on [Tauri](https://tauri.app)
(MIT / Apache-2.0); their licences ship in `node_modules` and the Cargo
registry. The harness marks in the terminal are drawn for Solyra, not the
vendors' logos.

## Design references (no code or artwork copied)

- pearOS (`pearOS-archlinux/pkgbuilds`): dock measurements, the Tahoe dock
  skin's glass recipe and its prism shader's parameters, used as numbers.
- Droppy (`1of1Adam/Droppy`): notch, HUD and lock-screen layout measurements.
- Apple's published macOS 26 screenshots: the look of Liquid Glass.

## Shell stack (2026-11-26)

- **Hyprland** (BSD-3-Clause): the compositor; `shell/hypr/hyprland.conf` is ours.
- **Quickshell** (LGPL-3.0): the QML shell runtime; `shell/qs/*.qml` is ours.
- **Qt 6** (LGPL-3.0): through Quickshell.
- **zgx** and **zgx-terminal** (MIT, zeronsh): the terminal UI and emulator
  glue (`alacritty_terminal` Apache-2.0, `portable-pty` MIT). Pinned at
  `699ae02e`. The chat engine follows zeron's (MIT) documented stream-json
  recipe; no zeron UI (GPUI) code is used.
- **lrclib.net**: the lock screen fetches synced lyrics from its public API
  at runtime. No lyrics are stored in the repository.
- **Droppy** (getdroppy.app): design reference for the notch and lock screen.
  No code or artwork copied.
- **liquid-gel** (pearOS, GPL-3.0): reference for the glass plugin
  (`plugins/solyra-glass`, GPL-3.0, kept separate from the MIT-derived
  terminal).

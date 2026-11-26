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

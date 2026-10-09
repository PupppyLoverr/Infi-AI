# cosmos-kit

`cosmos/kit` is the component set every CosmosOS app builds its chrome from. It sits on
`cosmos-uitk` (Wayland window + software rasterizer) and `cosmos-theme` (tokens). Apps do
not use raw egui widgets for chrome: toolbars, sidebars, lists, controls and menus all
come from the kit.

## Theme resolution
`Kit::get(ctx)` returns the frame's palette and accent. It re-reads
`~/.config/cosmos/config.json` (or `$COSMOS_CONFIG`) whenever the file's mtime changes,
so a light/dark or wallpaper-accent change restyles running apps on their next frame.
Every component draws its rest, hover, pressed, focus (2px accent ring) and disabled
(40% opacity) states from it.

## Metrics
| Token | Value |
|---|---|
| Window material | `palette.window` (dark alpha 0.90, light 0.92) |
| Unified toolbar | 52px (`layout::TOOLBAR_H`), 12px side padding, hairline below |
| Toolbar button | 28px, 18px icon, 6px radius |
| Sidebar | 220px, 28px rows, 16px accent icon |
| List/table rows | 28px (`cosmos_theme::ROW_H`) |
| Grid tile | 96px wide, 64px thumbnail, 2-line label |
| Status bar | 24px |
| Controls | 28px tall, 8px radius |
| Toggle | 38×22 track, 18px knob |
| Cards / grouped rows | 12px radius, 40px rows (52px with a detail line) |
| Menus / popovers / sheets | 14px radius, E2 shadow |

## Components
- `layout::AppWindow`: `.toolbar()`, `.sidebar()` and `.status()` sections around a
  `.show(ui, body)`.
- `layout::toolbar_button`, `toolbar_title`, `toolbar_spacer`.
- `controls::segmented` (text) and `segmented_with` (icon segments).
- `controls::search_field`: magnifier, hint, clear button.
- `layout::sidebar_section`, `sidebar_item`.
- `layout::Table` (`Column` widths, 0 = flex): `header()`, plus `row()` with an optional
  leading icon and ellipsized cells.
- `layout::grid_tile`.
- `layout::status_text`.
- `layout::card`, `card_frame`; `layout::group` with `Group::row`, `row_detail` and
  `custom` (System Settings-style grouped card).
- `controls::button` with `ButtonKind::{Primary, Secondary, Plain, Destructive}`.
- `controls::text_field` (with password masking), `toggle`, `slider`.
- `layout::dropdown`, `popover`, `menu_frame`, `menu_item`, `menu_separator`.
- `layout::sheet` (modal over a dimmed backdrop).
- `layout::tab_strip`, which returns `TabEvent::{Select, Close, New}`.

## Icons
The set in `kit/icons/*.svg` is original: a 24px grid, 1.5px round stroke, white. At
build time `kit/build.rs` rasterizes each SVG to a 2× PNG with resvg and generates the
`Icon` enum that embeds them. At runtime each icon is uploaded once and tinted when
painted, so one asset serves every theme and state. To add an icon, drop an SVG in
`kit/icons/` and its `Icon::CamelName` variant appears.

## Gallery
`cosmos-kit-gallery` shows every component and icon in one window. Point `COSMOS_CONFIG`
at a config with `"appearance": "light"` or `"dark"` to review each theme. Its
screendumps are `docs/evidence/kit-gallery-{dark,light}.png`.

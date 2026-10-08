# Config + theming

All CosmosOS settings are plain text under `~/.config/cosmos/` AND
live-editable through `system.settings.*` (the compositor persists them
and hot-reloads).

## Keys you can drive

- `dark` (bool) — dark/light theme, applied to every surface instantly.
- `accent` (string preset) — the single accent colour; presets ship in
  the compositor (blue default + alternates). Chrome stays neutral —
  never hard-code colours into generated content.
- `dock_side` ("left"|"bottom"|"right") — the dock rail position.
- `tiling` (bool) — dynamic master+stack tiling vs floating.
- `tier` ("lite"|"balanced"|"ultra") — performance tier (Lite disables
  motion/blur for weak GPUs).

Call `system.settings.get` for the full live map — it's the source of
truth.

## Fonts & design language

Inter for UI text, JetBrains Mono for code/keys. Cards use a 12px
radius (dock 15px), hairline borders, soft shadows. Match that language
when generating UI-facing content (widgets, launcher text).

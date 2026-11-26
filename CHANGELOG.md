# Changelog

## solyra-term – harness chat, browser, agents panel (done)
- Chat mode: Ctrl+Shift+M switches any pane between the raw terminal and a
  claude stream-json harness chat (bubbles, input pill, session cost in the
  status line, desktop notification when a turn finishes); the shell stays
  alive underneath either way.
- Built-in browser: Ctrl+Shift+B switches a pane to a WebKitGTK web view with
  an address row — real pages rendered in-process; terminal and browser share
  workspaces and splits.
- Agents side panel (Ctrl+Shift+P): the ADE surface — live task cards from
  the solyra-agent runtime protocol with approve/deny decisions written back
  through the decisions channel.
- Fixed: user bubbles no longer wiped by reader snapshots (shared
  conversation state); zgx apps need a dbus session bus in nested test rigs.


## M5–M7 – Island policy, a11y, overlays (done)
- Protocol v2: `set_intensity(refraction, frost, contrast)` accessibility
  scales (lens + frost + contrast in the material pass).
- Dynamic Island: hover +6 px, critically damped springs, content cross-fade,
  permission > question > HUD > shelf > message > media > activity, secondary
  live-activity badge; approvals never dropped.
- Settings › Appearance: increased contrast, refraction intensity, blur
  intensity; panel writes `~/.config/solyra/glass.kdl` (optional include).
- Menus, Control Centre, notifications, agents, about and dock context menus
  send popover/dark presets. `solyra-ui` titlebars/sidebars use a lighter
  glass fill over the window-rule blur.

## M4 – Linear-light blending (done)
- Kawase blur averages in linear light (decode on first downsample);
  material pass encodes back to sRGB at output. Glass-effect elements only.

## M3 – Compositor material pass (done)
- Protocol: presets dark/dock/island/popover/window added to solyra_glass_v1.
- Compositor shader: per-shape material — adaptive tint (in-shader luminance,
  no readback), frost, rim light, specular, inner shade.
- Panel: surfaces mapped to presets; CPU glass paint skipped when the
  compositor owns the material (fallback kept).

## Unreleased

### Liquid Glass (M2)

- Compositor: padded backdrop capture for `ext-background-effect-v1` / solyra
  glass surfaces so dual-Kawase blur and rim lensing no longer clamp at the
  texture edge (`comp/src/render_helpers/framebuffer_effect.rs`).
- Test wallpaper: `assets/wallpapers/stripes-1920x1080.png` for live A/B shots.
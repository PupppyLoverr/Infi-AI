# Changelog

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
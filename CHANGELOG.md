# Changelog

## Unreleased

### Liquid Glass (M2)

- Compositor: padded backdrop capture for `ext-background-effect-v1` / solyra
  glass surfaces so dual-Kawase blur and rim lensing no longer clamp at the
  texture edge (`comp/src/render_helpers/framebuffer_effect.rs`).
- Test wallpaper: `assets/wallpapers/stripes-1920x1080.png` for live A/B shots.
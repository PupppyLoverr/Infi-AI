# Handoff: finish Solyra's Liquid Glass + Dynamic Island

Paste everything below into a new agent session on `PupppyLoverr/Infi-AI`
(branch `claude/charming-goodall-iuca3o`; `main` is at the same commit).

---

You are continuing a production-grade Liquid Glass + Dynamic Island effort on
Solyra OS. Read this whole brief before editing. Reuse what exists; do not
rewrite working subsystems.

## Architecture (already audited, verified)

- **Compositor**: `comp/` is a niri fork (Smithay, GLES2). Blur is dual-kawase
  (`comp/src/render_helpers/blur.rs`, `shaders/blur_down.frag`/`blur_up.frag`).
  Per-surface background effects come from `ext-background-effect-v1`
  (`comp/src/handlers/background_effect.rs`) and layer rules. Two paths:
  `xray` (pre-blurred shared wallpaper buffers, `render_helpers/xray.rs`) and
  non-xray (`framebuffer_effect.rs`, captures the real scene behind). Solyra's
  panel surfaces use `xray false`, i.e. the real scene.
- Both paths draw through `postprocess_and_clip` =
  `shaders/refract_surface.frag` + `rounding_alpha.frag` + `postprocess.frag`.
  `refract_surface.frag` holds the **lens**: inward sampling along the shape
  normal over a rim band (1.5 × strength), zero outside the shape, slight RGB
  split. Uniforms `lens` (shape box normalised to clip geo) and `refraction`
  (strength px, corner radius px) come from `RenderParams::lens()` in
  `background_effect.rs`.
- **Rule options added** (`comp/niri-config/src/appearance.rs`,
  `BackgroundEffectRule`): `refraction`, `refraction-radius`, `blur-passes`,
  `blur-offset`. Presets live in `base/solyra-config.kdl` (dock = clear glass:
  2 passes / offset 2.5 / refraction 11; global blur 3 / 3).
- **Panel**: `shell/panel` is an SCTK layer-shell client with a software
  canvas (`canvas.rs`, premultiplied ARGB8888 shm). It paints tint, rim and
  highlights; the compositor does blur and lensing behind. Shared material:
  `material.rs` (`apple_glass`, `liquid_glass`, `pear_glass`, `smoked_glass`,
  `slab`/`paint`), `Canvas::rim_light` (directional specular), `Canvas::glyph`
  (Solar icons, `glyphs.rs`). Island: `island.rs` (Content, Anim, target, draw);
  Spotlight: `surfaces.rs` (`island_spot_*`, `draw_island_spotlight`); dock:
  `dock.rs` (bottom + `draw_vertical` for left/right); widgets: `widgets.rs`.
- **Live visual testing** (works in a cloud container, no GPU):
  `tools/live-shot.sh <name> <wallpaper.png> "<shell.conf>" [wait] [cmd]` runs
  the real compositor nested in headless weston with Mesa software GL, the
  real panel and swaybg, and saves a compositor screenshot. Needs
  `apt-get install weston swaybg libseat-dev libpipewire-0.3-dev libudev-dev
  libinput-dev libgbm-dev libxkbcommon-dev libdisplay-info-dev libclang-dev
  libegl1-mesa-dev libwayland-dev libpango1.0-dev libsystemd-dev libdbus-1-dev`,
  then `cargo build` in `comp/` and `shell/panel/`. Use a high-contrast stripe
  wallpaper for A/B checks (diff with `refraction 0`). Previews for quick
  iteration: `SOLYRA_RENDER_DIR=dir cargo test` in `shell/panel`
  (`preview.rs`, `compositor_lens` mirrors the shader).

## Verified so far (live screenshots)

Dock transmits the real scene (stripes recognisable), lensing confined to the
rim, no streaks; widgets are regular/tinted glass; Spotlight field grows from
the island at the top edge with the island hidden; works over purple, beach
and light ribbon wallpapers.

## Remaining work, in priority order

1. **Lens shape for multi-shape surfaces.** The lens box is the bounding box
   of the surface's blur region, so a surface with two shapes (widgets: two
   cards; island Spotlight: field + card) gets one box. Either send one
   surface per shape, or extend the shader with up to 4 lens boxes (uniform
   array) derived from connected components of the region. Then enable
   `refraction` on `solyra-widgets`.
2. **Adaptive luminosity/tint.** Panel tints are fixed per theme. Sample the
   backdrop luminance (the panel already computes `prefs.wall_tint` from the
   wallpaper; better: a compositor-side average of the captured region passed
   as a uniform) and adapt tint alpha and ink so bright backdrops do not wash
   out icons and dark ones do not swallow the glass.
3. **Regular vs clear as real material params** end to end: add a
   `GlassMaterial { variant, tint, tint_strength, transmission, blur, refraction,
   edge_width, edge_intensity, reflection, radius, inner_shade, shadow }`
   in `material.rs`, presets (`clear`, `regular`, `dark`, `dock`, `island`,
   `popover`, `window`), map each to a layer-rule preset in the KDL, and make
   every surface use it instead of ad-hoc calls.
4. **Island state machine and animation.** States exist (Idle, Media,
   Shelf, Glance, Attention/Question, Hud, Activity, Message) with spring-ish
   interpolation of w/h/r in `island::Anim`. Add: hover response (slight
   width +6 px, no glow), crossfade content during morphs, a priority/
   aggregation policy for simultaneous activities (approvals > questions >
   HUD > media > activity; show a small secondary indicator rather than
   dropping one; never drop approvals), and verify reduced motion.
5. **Accessibility settings** (Settings › Appearance): reduced transparency
   (exists as "Solid surfaces"), reduced motion (exists), add increased
   contrast, refraction intensity and blur intensity. The panel would write
   `~/.config/solyra/shell.conf`; the compositor reads KDL, so add a small
   include or have the panel rewrite a generated `glass.kdl` included by
   `solyra-config.kdl` and call `niri msg action load-config-file`.
6. **Menus, popovers, titlebars**: menus (`menu.rs`) and `solyra-ui` titlebars
   still use the older look; move them onto the shared material.
7. **Performance on Intel iGPU** (needs the real VM or hardware, not this
   container): measure frame time during window drags, dock hover and island
   morphs with refraction on/off; consider caching the blurred capture when
   the region is undamaged.
8. **Colour space**: niri blends in gamma space; check for dark fringes at
   translucent rounded edges in live shots; fix premultiplication if any.

## Rules

- Verify every visual change with `tools/live-shot.sh` (real compositor), not
  only previews. Compare before/after crops; A/B with the effect off.
- Run `cargo test` in `shell/panel`, `crates/solyra-conf`, `apps`, and
  `cargo test -p niri-config` in `comp/`; validate the KDL by building the
  compositor and running it (`niri validate -c base/solyra-config.kdl` or the
  live harness log).
- Keep commits small, explain each root cause in the message. The owner wants
  `origin/main` kept in step with the dev branch (fast-forward).
- No Apple or pearOS artwork/code in the repo; credits go in
  `THIRD_PARTY_NOTICES.md`.

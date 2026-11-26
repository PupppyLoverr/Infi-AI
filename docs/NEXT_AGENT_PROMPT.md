# Prompt for the next agent: Solyra OS Liquid Glass + Dynamic Island

Copy everything below into a new agent session.

---

## 0. Who you are and what this is

You are a senior Linux graphics engineer, Rust systems architect, Wayland
compositor specialist and desktop UI/UX engineer continuing work on **Solyra
OS** (repo `PupppyLoverr/Infi-AI`). Work on branch
`claude/charming-goodall-iuca3o` and keep `origin/main` fast-forwarded to it
(the owner wants everything on main). Start from main at commit `0eeae8f`
(M1: solyra_glass_v1) or later.

Read, in this order, before touching anything:
1. `docs/handoff-liquid-glass.md` (architecture audit, M1 status + evidence, roadmap).
2. `THIRD_PARTY_NOTICES.md`, `CHANGELOG.md`.
3. `comp/resources/solyra-glass-v1.xml`, `comp/src/protocols/solyra_glass.rs`,
   `comp/src/render_helpers/{background_effect,framebuffer_effect,xray,blur}.rs`,
   `comp/src/render_helpers/shaders/{refract_surface.frag,postprocess.frag,blur_down.frag,blur_up.frag,mod.rs}`.
4. `shell/panel/src/{main.rs,material.rs,canvas.rs,island.rs,dock.rs,surfaces.rs,widgets.rs,glass_proto.rs,preview.rs,lock.rs,hud.rs}`.
5. `base/solyra-config.kdl` (layer rules, blur presets, key bindings), `crates/solyra-conf/src/look.rs` (all shell options).

## 1. The owner's goal (non-negotiable)

Reproduce the **appearance and behaviour of macOS Liquid Glass** (macOS Tahoe /
macOS 26) as closely as technically possible, plus a **Dynamic Island**
modelled on Apple's (and the reported MacBook Pro concept), in an original
Solyra identity. Not generic glassmorphism. The ultimate acceptance criterion
is **visual fidelity to the macOS reference**, not compiling, not correct
protocol messages, not "a working blur shader".

Reference material the owner supplied (ask them for the files if you need them;
they were chat images): macOS 26 MacBook with Spotlight capsule over a
cream/taupe ribbon wallpaper and a light glass dock; macOS 26 Golden Gate
desktop with Finder, Photos, a dark Siri glass panel and a right-side dock;
a light glass pill with trash/folder/reply glyphs showing a lit rim over
ribbons; Apple's Dynamic Island media player (black, artwork, progress,
controls); the MacBook-notch Spotlight where a **black field grows out of the
notch** and the **results sit on a separate dark glass card** below it (the
owner called this "the dynamic island glass great"). Also pearOS screenshots
(dock, top bar, notch) and the Droppy app (getdroppy.app; source
`1of1Adam/Droppy`) for notch/HUD/lock-screen features.

Observable characteristics you must reproduce:
- Real-time refraction / lensing of the **actual composited backdrop** (windows included), restrained, smooth, geometry-aware, strongest at curved rims, flat regions undistorted, no ripples/fish-eye/water.
- Recognisable background colours and large shapes through glass (no muddy uniform blur).
- Convincing edge curvature, thin controlled edge illumination, subtle specular highlights, subtle inner shading and depth; no thick white outlines, neon borders, bloom, plastic or chrome looks.
- Adaptive tint, luminosity and contrast from the content behind each surface; separate **clear** and **regular** materials that differ by real parameters.
- Different material responses for small controls, the dock, floating panels and expanded surfaces.
- Smooth, coherent shape morphing where geometry, lensing, reflections and content stay synchronized.
- Correct transparency, antialiasing and colour handling: no dark fringes, no artificial outlines.

## 2. Hard rules from the owner

- **Keep the architecture**: Rust layer-shell panel (`shell/panel`, CPU canvas) + `solyra-comp` (niri 26.4.0 fork, Smithay 0.7, GLES2). The **compositor owns the shared optical material** (it has the real backdrop). Do not migrate to Tauri/React or another toolkit and do not rewrite the compositor unless *measured* evidence demands it; report first.
- Do not change niri's window management or layout behaviour.
- Preserve every working feature: dock (bottom/left/right, magnification, autohide, styles Floating/Solyra/Taskbar/Minimal, Downloads, Trash, tooltips, launch bounce), menu bar (App/File/Edit/View/Go/Window/Help, real actions), Dynamic Island (idle, glance, media with seek, file shelf with drag-and-drop, agent approvals and questions, HUDs for volume/brightness/charging/Focus/unlock), Spotlight (in-island and floating), lock screen (notch padlock spring, Droppy media panel, 4 layouts), desktop widgets (calendar, weather via Open-Meteo), Control Centre, notifications, setup wizard, Settings, Files, Terminal, Browser.
- Root cause first: trace and prove, then fix the smallest reliable thing. Do not assume a shader works because it compiles; verify inputs, coordinates, alpha, colour space, compositing order.
- **Verify on real pixels**: every visual change must be checked with `tools/live-shot.sh` (real compositor), with before/after captures of the *same* scene, scale, wallpaper and config. Previews (`SOLYRA_RENDER_DIR=... cargo test` in `shell/panel`) are only for quick iteration. Distinguish expected differences from regressions; A/B with the effect switched off.
- Never invent APIs, results or benchmark numbers. Label findings confirmed / probable / unverified. No performance claims without measurements on real hardware (Intel iGPU, 16 GB RAM is the target).
- Accessibility is required: reduced transparency, reduced motion, increased contrast, refraction and blur intensity; transparency must never be mandatory.
- No Apple, Microsoft or pearOS artwork or code in the repo. pearOS/Droppy are references only (numbers, behaviour). Credits go in `THIRD_PARTY_NOTICES.md` (Solar icons CC BY 4.0 already credited).
- Commits: author stays the configured identity; end every message with
  `Co-Authored-By: PupppyLoverr <165215920+PupppyLoverr@users.noreply.github.com>` and
  `Co-Authored-By: Claude <noreply@anthropic.com>` (plus the session trailer your harness gives). **Never put the owner's private email in a commit.** Push the dev branch and fast-forward `origin/main` (`git push origin HEAD:main`) after each verified milestone.
- No phone notifications or emailed scheduled tasks. Be economical with usage: targeted reads (`grep -n`, `sed -n`), no re-reading large files, background long builds.
- Stop and report if the renderer or protocol APIs make a planned design materially different, before expanding scope.

## 3. Verified architecture (do not re-derive; spot-check only)

- Panel → compositor channels: layer-shell namespaces (`solyra-dock.glass`, `solyra-widgets`, `solyra-notch.solid`, `solyra-launcher-island`, `solyra-control`, ...) select KDL layer rules; `ext-background-effect-v1` blur regions (`set_blur`, `material::blur_rects` row strips); **`solyra_glass_v1`** (M1) analytic shapes (`set_glass` in `shell/panel/src/main.rs`, client code `glass_proto.rs`); niri IPC; panel's own `solyra-shell.sock` (CLI `solyra-panel <cmd>`: toggle-launcher, toggle-island, hud <kind>, media <action>, lock, dock <style>, notify, ...).
- Compositor effect path: `background_effect.rs::render_for_tile` → `render_params_for_tile` (blur region → `TransformedRegion`, whose `iter()` yields **absolute** rects) → non-xray `FramebufferEffectElement` (`capture_framebuffer` blits exactly the element's dst rect into an `Abgr8888` texture, **no padding**) → `blur.rs` dual-Kawase (texture 0 full size, halves per pass; per-surface `blur-passes`/`blur-offset` rule options) → draw with `postprocess_and_clip` = `refract_surface.frag` + `rounding_alpha.frag` + `postprocess.frag`. xray path (`xray.rs`) is the same final shader on pre-blurred wallpaper buffers. Shell surfaces use `xray false`.
- `refract_surface.frag`: if `shape_count > 0` → union of up to 4 analytic rounded boxes (`shape0..3`, `shape_radius`, `shape_lens`), 1 px AA clip (`glass_cov`), lens offset `-n * f * k / per_tex` sampled **inward**, band `max(k*1.5, 6)`, falloff `(1+d/band)^2`, RGB split 1.25/0.75; else fallback single box from the blur region's bounds (`lens`, `refraction` uniforms). Per-surface rule options: `refraction`, `refraction-radius`, `blur-passes`, `blur-offset`, `saturation`, `noise`, `xray`, `blur`.
- Presets in protocol: clear (lens ×1.0), regular (×0.6), solid (×0, clip only). Default strength without a rule: 10 px.
- Panel paints tint/rims on CPU: `material::apple_glass` (smoke/milk tint + `Canvas::rim_light` directional key top-left/back bottom-right + hairline), `smoked_glass`, `pear_glass` (pearOS Tahoe skin numbers: 15 % body, 1 px 30 % rim, prism glow), `liquid_glass`. Glyphs: Solar via `Canvas::glyph` (`assets/glyphs`, `bake_solar.py`).
- All blending is 8-bit gamma-encoded (`Abgr8888`, no sRGB decode). Panel surfaces are fully redrawn and fully damaged per frame (`present()`); island animates via frame callbacks on its own surface (480×214 logical).
- Island: `island.rs` (`Content` enum: Idle, Activity, Attention(Ask), Message, Media, Shelf, Hud; `target()` sizes from Droppy: island 210×37 at y 4, notch 180×32, expanded 450 wide; `Anim` interpolates x/y/w/h/r; `island_content()` priority in `main.rs`: attention > HUD (1.6 s) > shelf tab > message > media > activity > idle). Spotlight in island: `surfaces.rs::island_spot_*`, black field + separate glass card, island hidden while open, layer surface exclusive zone -1.

## 4. Environment setup for live verification (cloud container, no GPU)

```
apt-get install -y weston swaybg libseat-dev libpipewire-0.3-dev libudev-dev libinput-dev \
  libgbm-dev libxkbcommon-dev libdisplay-info-dev libclang-dev libegl1-mesa-dev \
  libwayland-dev libpango1.0-dev libsystemd-dev libdbus-1-dev
(cd comp && cargo build)            # ~3-25 min first time; background it
(cd shell/panel && cargo build)
tools/live-shot.sh <name> <wallpaper.png> "<shell.conf lines with \n>" [wait-s] ["extra shell cmd"]
# e.g. tools/live-shot.sh dock assets/wallpapers/silk-1920x1080.png "" 10
#      tools/live-shot.sh spot  stripes.png "theme=light\n" 10 "sleep 4; shell/panel/target/debug/solyra-panel toggle-launcher"
# env SOLYRA_GLASS=0 disables solyra_glass_v1 in the panel (fallback A/B)
```
It runs the real compositor nested in headless weston with Mesa software GL
(`XDG_RUNTIME_DIR=/tmp/sxr`, short path needed for the IPC socket), the real
panel and swaybg, and saves a compositor screenshot to `$LIVE_DIR`
(default `/tmp/solyra-live`). Make a high-contrast test wallpaper (diagonal
colour stripes) for A/B work; also test: dark purple Silk
(`assets/wallpapers/silk-1920x1080.png`), light ribbons
(`silk-light-1920x1080.png`), beach (`beach-1920x1080.png`), a colourful
detailed image, a plain light background, and overlapping/moving windows
(`weston-terminal` inside the nested session). Software GL says nothing about
real GPU performance.

## 5. Work to do, in order (each milestone: implement → test → live A/B → commit → push dev + main)

### M2 – Padded backdrop capture
Problem (confirmed in source): `capture_framebuffer` blits only the element's
rect; Kawase taps and lens samples clamp at the texture edge, so rims are
biased and the outside of the glass cannot bend in. Capture a margin of about
(blur radius + max lens strength) around the element, clamped to the output,
and adjust `input_to_geo`/texture coordinates so sampling (inward *and*
outward) stays inside valid data. Handle partial-damage crops (probable
shimmer: a partial repaint blurs only the damaged crop). Acceptance: on the
stripes A/B, rims show the outside scene bending in, no edge halos/streaks,
corners continuous, no change for non-glass surfaces; measure blit cost.

### M3 – Compositor material pass (the biggest fidelity step)
Move tint, adaptive luminosity/contrast, rim light, specular and inner shade
from the panel's CPU paint into the compositor shader for surfaces that send
`solyra_glass_v1` shapes. Backdrop luminance: sample the smallest blur level
(or a 1×1 mip) per shape, no GPU→CPU readback. Extend the protocol (version 2,
backward compatible) or presets so each shape carries: tint colour + strength,
transmission, frost, lens strength, edge width/intensity, reflection, inner
shade. Define real presets: `clear`, `regular`, `dark`, `dock`, `island`,
`popover`, `window` (see the table in `docs/handoff-liquid-glass.md`). Then
remove `apple_glass`/rim painting from protocol surfaces in the panel (keep it
as the fallback when the protocol is absent). Acceptance: legible text/icons
over dark purple, beach, stripes, white; the dock picks up the wallpaper's hue
without becoming opaque; no muddy grey; the purple wallpaper tints the dock
and island subtly; panel CPU per frame measured before/after.

### M4 – Linear-light blending for glass effect elements only
Decode sRGB→linear in the blur passes and the material pass, encode back at
output, only for background-effect elements. Do not change ordinary app
surfaces. Acceptance: black/white stripe blur averages to correct mid
luminance; no dark fringes on translucent rounded edges; overlapping glass
surfaces correct.

### M5 – Dynamic Island interaction model (panel)
State machine: Idle, Hover (≈ +6 px width within ~120 ms, no glow), Expanded
media, File shelf, Clock/glance, Agent approval, Agent question, HUD, multiple
simultaneous activities. Priority: approval > question > HUD (timed) >
message > media > activity > glance/idle; lower-priority live activity shows
as a small secondary indicator/badge, never dropped; **permission requests
are never silently discarded** and stay until answered. Animation: critically
damped springs for w/h/r (no overshoot/bounce), content crossfade in the last
~40 % of a morph, stable layout for progress/timestamps, reduced motion,
no unexpected keyboard focus (OnDemand only in reply mode). The island shape is
already sent each animation frame via `set_glass` with the commit; verify
mid-morph frames live (capture during toggle-island) that glass and content
never separate. Island idle stays near-black, softly reflective (Apple), with
the glass material optional.

### M6 – Accessibility and settings
Settings › Appearance (apps/solyra-settings): reduced transparency (exists as
"Solid surfaces"), reduced motion (exists), increased contrast, refraction
intensity, blur intensity, dark/light/tinted material appearance. The
compositor reads KDL: have Settings write a generated `glass.kdl` included by
`base/solyra-config.kdl` and run `niri msg action load-config-file`, or carry
the values in the protocol (v2). Every surface must respect them.

### M7 – System-wide consistency
Move menus (`menu.rs`), popovers, Control Centre, notifications, lock-screen
panels, calendar/weather widgets and `apps/solyra-ui` titlebars onto the
shared presets. Hierarchy: primary interactive = clear; information-dense =
regular/dark; system-critical prompts = high contrast. Ordinary apps keep
window-rule blur; only shell surfaces get lensing unless an app opts in.
Files (Finder-like: inset glass sidebar, capsule toolbar groups) and Settings
are zgx apps (`apps/`), wgpu-based, no layer-shell.

### M8 – Performance (needs the VMware VM or real Intel hardware)
Benchmarks: idle desktop, dock hover/magnification, island expansion, media
playback, many glass surfaces, moving overlapping windows, wallpaper change,
software-rendering fallback. Record frame times (niri has tracy spans), CPU,
GPU where available, memory, artefacts. Optimise by measured benefit: shared
backdrop/blur reuse, reduced-resolution blur, damage-aware updates, panel
partial damage instead of full-surface redraw, fewer per-frame allocations.

## 6. Visual validation matrix (every milestone)

Scenes: dark purple, bright beach/sky, colourful detailed, light, high-contrast
stripes, overlapping and moving windows. Components: dock (bottom, left,
right), compact island, expanded media island, file shelf, glance, agent
approval, agent question, HUD, Spotlight (in-island and floating), calendar,
weather, Control Centre, menus, a Files and a Settings window. Inspect:
background visibility, blur quality, refraction, shape accuracy, edge light,
tint adaptation, colour-space artefacts, corner clipping, animation
continuity, legibility, stability. For each, put the Solyra capture next to a
macOS Tahoe reference of an equivalent scene and **list the remaining
differences explicitly**; never conclude with "looks more glassy".

## 7. Deliverable format after each milestone

Changed-file list, design summary, implementation summary, test results
(`cargo test` in `shell/panel`, `cargo test --lib` and `-p niri-config` in
`comp`, `crates/solyra-conf`, `apps` where touched; `tools/live-shot.sh`
evidence), before/after screenshots (same scene), measured numbers only,
known limitations, then commit + push dev branch + fast-forward main and
update `docs/handoff-liquid-glass.md` and `CHANGELOG.md` with status for the
next agent.

## 8. Other context worth knowing

- CI: `.github/workflows/disk.yml` (pushes to `claude/**`) builds the
  compositor, panel and apps in a Fedora container, runs tests, builds the
  bootc image, a raw disk and a **VMware vmdk** (`solyra-vmware` artifact) and
  boot-smoke-tests to login. Newer pushes cancel older runs. The owner tests
  in VMware; give them the artifact link and steps when a run is green.
- Defaults: dock Floating bottom, magnification off (macOS default), notch
  Dynamic Island with Solid (black) material, Spotlight in notch on, menu bar
  Glass (fully see-through, pearOS style), theme dark, wallpaper Silk.
  Setup wizard (`apps/solyra-setup`) asks theme, top bar, notch, dock style,
  dock position.
- User icons: PNGs in `~/.local/share/solyra/icons/` override bundled icons
  (never bundle third-party/macOS icon-gallery art).
- Known open items besides the milestones: lens is faint on regular presets
  over heavy frost (tune in M3), multi-shape surfaces with more than 4 shapes
  fall back to the first 4, the bar uses no blur (pearOS look) by design.

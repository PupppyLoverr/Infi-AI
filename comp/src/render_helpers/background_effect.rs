use std::sync::{Arc, Mutex};

use niri_config::CornerRadius;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Scale, Size};
use smithay::wayland::compositor::{with_states, SurfaceData};
use wayland_server::protocol::wl_surface::WlSurface;

use crate::handlers::background_effect::get_cached_blur_region;
use crate::niri_render_elements;
use crate::render_helpers::blur::BlurOptions;
use crate::render_helpers::damage::ExtraDamage;
use crate::render_helpers::framebuffer_effect::{FramebufferEffect, FramebufferEffectElement};
use crate::render_helpers::xray::{XrayElement, XrayPos};
use crate::render_helpers::RenderCtx;
use crate::utils::region::TransformedRegion;
use crate::utils::surface_geo;

#[derive(Debug)]
pub struct BackgroundEffect {
    nonxray: FramebufferEffect,
    /// Damage when options change.
    damage: ExtraDamage,
    /// Corner radius for clipping.
    ///
    /// Stored here in addition to `RenderParams` to damage when it changes.
    // FIXME: would be good to remove this duplication of radius.
    corner_radius: CornerRadius,
    blur_config: niri_config::Blur,
    options: Options,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Options {
    pub blur: bool,
    pub xray: bool,
    pub noise: Option<f64>,
    pub saturation: Option<f64>,
    /// Liquid-glass lensing: (strength, corner radius) in logical pixels.
    pub refraction: Option<(f64, f64)>,
    /// Per-surface blur strength (material presets): passes and offset.
    pub blur_passes: Option<u8>,
    pub blur_offset: Option<f64>,
}

impl Options {
    fn is_visible(&self) -> bool {
        self.xray
            || self.blur
            || self.noise.is_some_and(|x| x > 0.)
            || self.saturation.is_some_and(|x| x != 1.)
    }
}

/// Analytic glass shapes as shader uniforms.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlassUniforms {
    pub boxes: [[f32; 4]; 4],
    pub radius: [f32; 4],
    pub lens: [f32; 4],
    /// Material preset id per shape (solyra_glass_v1 preset).
    pub preset: [f32; 4],
    pub count: f32,
    /// (refraction_scale, frost_scale, contrast_scale, unused)
    pub intensity: [f32; 4],
}

impl GlassUniforms {
    pub fn uniforms(&self) -> [smithay::backend::renderer::gles::Uniform<'static>; 9] {
        use smithay::backend::renderer::gles::Uniform;
        [
            Uniform::new("shape0", self.boxes[0]),
            Uniform::new("shape1", self.boxes[1]),
            Uniform::new("shape2", self.boxes[2]),
            Uniform::new("shape3", self.boxes[3]),
            Uniform::new("shape_radius", self.radius),
            Uniform::new("shape_lens", self.lens),
            Uniform::new("shape_preset", self.preset),
            Uniform::new("shape_count", self.count),
            Uniform::new("mat_intensity", self.intensity),
        ]
    }
}

/// Render-time parameters.
#[derive(Debug)]
pub struct RenderParams {
    /// Geometry of the background effect.
    pub geometry: Rectangle<f64, Logical>,
    /// Effect subregion, will be clipped to `geometry`.
    ///
    /// `subregion.iter()` should return `geometry`-relative rectangles.
    pub subregion: Option<TransformedRegion>,
    /// Geometry and radius for clipping in the same coordinate space as `geometry`.
    pub clip: Option<(Rectangle<f64, Logical>, CornerRadius)>,
    /// Liquid-glass lensing (strength, radius), filled in by `render()`.
    pub refraction: [f32; 2],
    /// solyra_glass_v1 shapes in the same absolute space as `geometry`:
    /// (rect, corner radius, preset lens scale, preset id). Empty: fall back
    /// to `lens()`.
    pub shapes: Vec<(Rectangle<f64, Logical>, f32, f32, f32)>,
    /// Accessibility intensity: refraction, frost, contrast scales (1.0 = native).
    pub intensity: [f32; 3],
    /// Scale to use for rounding to physical pixels.
    pub scale: f64,
}

impl RenderParams {
    /// The glass shape for lensing: the bounding box of the effect subregion
    /// (or the whole geometry), normalised to the clip geometry.
    pub fn lens(&self) -> [f32; 4] {
        // `subregion.iter()` yields rects already translated by the surface
        // position (TransformedRegion::offset), i.e. in the same absolute
        // space as `geometry` and `clip`.
        let clip = self.clip.map_or(self.geometry, |c| c.0);
        let g = self.geometry;
        let (mut x0, mut y0, mut x1, mut y1) =
            (g.loc.x, g.loc.y, g.loc.x + g.size.w, g.loc.y + g.size.h);
        if let Some(sub) = &self.subregion {
            let mut first = true;
            for (tl, br) in sub.iter() {
                if first {
                    (x0, y0, x1, y1) = (tl.x, tl.y, br.x, br.y);
                    first = false;
                } else {
                    x0 = f64::min(x0, tl.x);
                    y0 = f64::min(y0, tl.y);
                    x1 = f64::max(x1, br.x);
                    y1 = f64::max(y1, br.y);
                }
            }
        }
        let (cw, ch) = (clip.size.w.max(1.), clip.size.h.max(1.));
        [
            ((x0 - clip.loc.x) / cw) as f32,
            ((y0 - clip.loc.y) / ch) as f32,
            ((x1 - x0) / cw) as f32,
            ((y1 - y0) / ch) as f32,
        ]
    }

    /// Shader uniforms for the analytic shapes: boxes normalised to the clip
    /// geometry, radii (px), lens strengths (px) and the count.
    pub fn glass_uniforms(&self) -> GlassUniforms {
        let mut u = GlassUniforms::default();
        let clip = self.clip.map_or(self.geometry, |c| c.0);
        let (cw, ch) = (clip.size.w.max(1.), clip.size.h.max(1.));
        // Without a refraction rule, analytic glass still lenses at this
        // default strength (a clear preset's).
        let strength = if self.refraction[0] > 0. {
            self.refraction[0]
        } else {
            10.
        };
        for (i, (r, radius, scale, preset)) in self.shapes.iter().take(4).enumerate() {
            u.boxes[i] = [
                ((r.loc.x - clip.loc.x) / cw) as f32,
                ((r.loc.y - clip.loc.y) / ch) as f32,
                (r.size.w / cw) as f32,
                (r.size.h / ch) as f32,
            ];
            u.radius[i] = *radius;
            u.lens[i] = strength * scale;
            u.preset[i] = *preset;
            u.count += 1.;
        }
        if u.count > 0.5 {
            u.intensity = [
                self.intensity[0],
                self.intensity[1],
                self.intensity[2],
                0.,
            ];
        }
        u
    }

    fn fit_clip_radius(&mut self) {
        if let Some((geo, radius)) = &mut self.clip {
            // HACK: increase radius to avoid slight bleed on rounded corners.
            *radius = radius.expanded_by(1.);

            *radius = radius.fit_to(geo.size.w as f32, geo.size.h as f32);
        }
    }
}

niri_render_elements! {
    BackgroundEffectElement => {
        FramebufferEffect = FramebufferEffectElement,
        Xray = XrayElement,
        ExtraDamage = ExtraDamage,
    }
}

impl BackgroundEffect {
    pub fn new() -> Self {
        Self {
            nonxray: FramebufferEffect::new(),
            damage: ExtraDamage::new(),
            corner_radius: CornerRadius::default(),
            blur_config: niri_config::Blur::default(),
            options: Options::default(),
        }
    }

    /// Damage the background effect, for example when a blur subregion changes.
    pub fn damage(&mut self) {
        self.damage.damage_all();
        self.nonxray.damage();
    }

    pub fn update_config(&mut self, config: niri_config::Blur) {
        if self.blur_config == config {
            return;
        }

        self.blur_config = config;
        self.damage.damage_all();
        self.nonxray.damage();
    }

    pub fn update_render_elements(
        &mut self,
        corner_radius: CornerRadius,
        effect: niri_config::BackgroundEffect,
        has_blur_region: bool,
    ) {
        // If the surface explicitly requests a blur region, default blur to true.
        let blur = if has_blur_region {
            effect.blur != Some(false)
        } else {
            effect.blur == Some(true)
        };

        let mut options = Options {
            blur,
            xray: effect.xray == Some(true),
            noise: effect.noise,
            saturation: effect.saturation,
            refraction: effect
                .refraction
                .filter(|x| *x > 0.)
                .map(|x| (x, effect.refraction_radius.unwrap_or(1000.))),
            blur_passes: effect.blur_passes,
            blur_offset: effect.blur_offset,
        };

        // If we have some background effect but xray wasn't explicitly set, default it to true
        // since it's cheaper.
        if options.is_visible() && effect.xray.is_none() {
            options.xray = true;
        }

        if self.options == options && self.corner_radius == corner_radius {
            return;
        }

        self.options = options;
        self.corner_radius = corner_radius;
        self.damage.damage_all();
        self.nonxray.damage();
    }

    pub fn is_visible(&self) -> bool {
        self.options.is_visible()
    }

    pub fn render(
        &self,
        ctx: RenderCtx<GlesRenderer>,
        ns: Option<usize>,
        mut params: RenderParams,
        xray_pos: XrayPos,
        push: &mut dyn FnMut(BackgroundEffectElement),
    ) {
        if !self.is_visible() {
            return;
        }

        if let Some(clip) = &mut params.clip {
            clip.1 = self.corner_radius;
        }
        params.fit_clip_radius();
        if let Some((strength, radius)) = self.options.refraction {
            params.refraction = [strength as f32, radius as f32];
        }

        let damage = self.damage.render(params.geometry);

        // Use noise/saturation from options, falling back to blur defaults if blurred, and
        // to no effect if not blurred.
        let blur = self.options.blur && !self.blur_config.off;
        let blur_options = blur.then(|| {
            let mut o = BlurOptions::from(self.blur_config);
            if let Some(p) = self.options.blur_passes {
                o.passes = p.clamp(1, 8);
            }
            if let Some(x) = self.options.blur_offset {
                o.offset = x;
            }
            o
        });
        let noise = if blur { self.blur_config.noise } else { 0. };
        let noise = self.options.noise.unwrap_or(noise) as f32;
        let saturation = if blur {
            self.blur_config.saturation
        } else {
            1.
        };
        let saturation = self.options.saturation.unwrap_or(saturation) as f32;

        if self.options.xray {
            let Some(xray) = ctx.xray else {
                return;
            };

            push(damage.into());
            xray.render(
                ctx,
                params,
                xray_pos,
                blur,
                noise,
                saturation,
                &mut |elem| push(elem.into()),
            );
        } else {
            // Render non-xray effect.
            let elem = self
                .nonxray
                .render(ns, params, blur_options, noise, saturation);
            push(elem.into());
        }
    }
}

fn render_params_for_tile(
    geometry: Rectangle<f64, Logical>,
    scale: f64,
    clip_to_geometry: bool,
    block_out: bool,
    blur_region: Option<Arc<Vec<Rectangle<i32, Logical>>>>,
    surface_geo: Rectangle<f64, Logical>,
    surface_anim_scale: Scale<f64>,
) -> Option<RenderParams> {
    // Effects not requested by the surface itself are drawn to match the geometry.
    let mut clip = true;

    let mut effect_geometry = geometry;
    let mut subregion = None;
    if let Some(rects) = blur_region {
        if rects.is_empty() {
            // Surface has a set, but empty blur region.
            return None;
        } else {
            // If the surface itself requests the effects, apply different defaults.
            clip = clip_to_geometry;

            // Use geometry-shaped blur for blocked-out windows to avoid unintentionally
            // leaking any surface shapes. We render those windows as geometry-shaped solid
            // rectangles anyway.
            if block_out {
                clip = true;
            } else {
                let mut surface_geo = surface_geo.upscale(surface_anim_scale);
                surface_geo.loc += geometry.loc;

                subregion = Some(TransformedRegion {
                    rects,
                    scale: surface_anim_scale,
                    offset: surface_geo.loc,
                });

                surface_geo = surface_geo
                    .to_physical_precise_round(scale)
                    .to_logical(scale);
                effect_geometry = surface_geo;
            }
        }
    }

    // This corner radius is reset to self.corner_radius in render().
    let clip = clip.then_some((geometry, CornerRadius::default()));

    Some(RenderParams {
        geometry: effect_geometry,
        subregion,
        clip,
        scale,
        refraction: [0., 0.],
        shapes: Vec::new(),
        intensity: [1., 1., 1.],
    })
}

/// Per-surface background effect stored in its data map.
struct SurfaceBackgroundEffect(Mutex<BackgroundEffect>);

impl SurfaceBackgroundEffect {
    fn get(states: &SurfaceData) -> &Self {
        states
            .data_map
            .get_or_insert(|| SurfaceBackgroundEffect(Mutex::new(BackgroundEffect::new())))
    }
}

pub fn damage_surface(states: &SurfaceData) {
    if let Some(effect) = states.data_map.get::<SurfaceBackgroundEffect>() {
        effect.0.lock().unwrap().damage();
    }
}

// Silence, Clippy
// A Smithay user is talking
#[allow(clippy::too_many_arguments)]
pub fn render_for_tile(
    ctx: RenderCtx<GlesRenderer>,
    ns: Option<usize>,
    geometry: Rectangle<f64, Logical>,
    scale: f64,
    clip_to_geometry: bool,
    surface: &WlSurface,
    surface_off: Point<f64, Logical>,
    surface_anim_scale: Scale<f64>,
    blur_config: niri_config::Blur,
    radius: CornerRadius,
    effect: niri_config::BackgroundEffect,
    should_block_out: bool,
    xray_pos: XrayPos,
    push: &mut dyn FnMut(BackgroundEffectElement),
) {
    with_states(surface, |states| {
        let background_effect = SurfaceBackgroundEffect::get(states);
        let mut background_effect = background_effect.0.lock().unwrap();

        let blur_region = get_cached_blur_region(states);
        let has_blur_region = blur_region.as_ref().is_some_and(|r| !r.is_empty());

        background_effect.update_config(blur_config);
        background_effect.update_render_elements(radius, effect, has_blur_region);

        if !background_effect.is_visible() {
            return;
        }

        let mut surface_geo = surface_geo(states).unwrap_or_default().to_f64();
        surface_geo.loc += surface_off;

        let Some(mut params) = render_params_for_tile(
            geometry,
            scale,
            clip_to_geometry,
            should_block_out,
            blur_region,
            surface_geo,
            surface_anim_scale,
        ) else {
            return;
        };
        // solyra_glass_v1: the committed shapes, in the same absolute space
        // as the blur sub-region (surface origin + shape * anim scale).
        if !should_block_out {
            if let Some(state) = crate::protocols::solyra_glass::committed(states) {
                let origin = surface_geo.upscale(surface_anim_scale).loc + geometry.loc;
                params.shapes = state
                    .shapes
                    .iter()
                    .map(|s| {
                        let loc = Point::from((s.x as f64, s.y as f64)).upscale(surface_anim_scale);
                        let size = Size::from((s.w as f64, s.h as f64)).upscale(surface_anim_scale);
                        (
                            Rectangle::new(origin + loc, size),
                            s.radius,
                            s.preset.lens_scale() * state.refraction_scale,
                            s.preset.id(),
                        )
                    })
                    .collect();
                params.intensity = [
                    state.refraction_scale,
                    state.frost_scale,
                    state.contrast_scale,
                ];
            }
        }

        let xray_pos = xray_pos.offset(params.geometry.loc - geometry.loc);
        background_effect.render(ctx, ns, params, xray_pos, push);
    });
}

#[cfg(test)]
mod glass_tests {
    use super::*;

    fn params(shapes: Vec<(Rectangle<f64, Logical>, f32, f32, f32)>) -> RenderParams {
        RenderParams {
            geometry: Rectangle::new(Point::from((100., 600.)), Size::from((400., 72.))),
            subregion: None,
            clip: None,
            scale: 1.,
            refraction: [0., 0.],
            shapes,
            intensity: [1., 1., 1.],
        }
    }

    #[test]
    fn no_shapes_means_fallback() {
        let p = params(Vec::new());
        assert_eq!(p.glass_uniforms().count, 0.);
        // The fallback lens is the whole geometry.
        assert_eq!(p.lens(), [0., 0., 1., 1.]);
    }

    #[test]
    fn shapes_are_normalised_to_the_clip_geometry() {
        // Two cards side by side inside the surface.
        let a = Rectangle::new(Point::from((100., 600.)), Size::from((196., 72.)));
        let b = Rectangle::new(Point::from((310., 600.)), Size::from((190., 72.)));
        let u = params(vec![(a, 22., 1., 0.), (b, 22., 0.6, 1.)]).glass_uniforms();
        assert_eq!(u.count, 2.);
        assert_eq!(u.boxes[0], [0., 0., 0.49, 1.]);
        assert!((u.boxes[1][0] - 0.525).abs() < 1e-6 && (u.boxes[1][2] - 0.475).abs() < 1e-6);
        assert_eq!(u.radius[..2], [22., 22.]);
        // No refraction rule: the default strength, scaled by preset.
        assert_eq!(u.lens[0], 10.);
        assert!((u.lens[1] - 6.).abs() < 1e-6);
    }

    #[test]
    fn the_rule_strength_wins_and_the_count_is_bounded() {
        let r = Rectangle::new(Point::from((100., 600.)), Size::from((10., 10.)));
        let mut p = params(vec![(r, 2., 1., 0.); 6]);
        p.refraction = [11., 22.];
        let u = p.glass_uniforms();
        assert_eq!(u.count, 4., "at most four shapes reach the shader");
        assert_eq!(u.lens[0], 11.);
    }
}

use smithay::{
    backend::renderer::{
        damage::{Error as OutputDamageTrackerError, OutputDamageTracker, RenderOutputResult},
        element::{
            solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement,
            texture::TextureRenderElement,
            utils::{
                ConstrainAlign, ConstrainScaleBehavior, CropRenderElement, Relocate,
                RelocateRenderElement, RescaleRenderElement,
            },
            AsRenderElements, Id, Kind, RenderElement, Wrap,
        },
        utils::CommitCounter,
        Color32F, ImportAll, ImportMem, Renderer,
    },
    desktop::{
        layer_map_for_output,
        space::{
            constrain_space_element, ConstrainBehavior, ConstrainReference, Space,
            SpaceRenderElements,
        },
    },
    output::Output,
    reexports::wayland_server::Resource as _,
    utils::{Logical, Point, Rectangle, Scale, Size},
    wayland::shell::wlr_layer::Layer as WlrLayer,
};

#[cfg(feature = "debug")]
use crate::drawing::FpsElement;
use crate::{
    drawing::{clear_color, PointerRenderElement, CLEAR_COLOR_FULLSCREEN},
    shell::{ssd, FullscreenSurface, WindowElement, WindowRenderElement},
};

// Drop shadows floating over the desktop come in two flavours: window
// chrome paints its own (ssd.rs), and floating shell cards get one from
// the compositor here — keyed per layer surface so the SDF repaint only
// runs when the card's size changes.
thread_local! {
    static LAYER_SHADOWS: std::cell::RefCell<
        std::collections::HashMap<u32, ssd::WindowShadow>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

smithay::backend::renderer::element::render_elements! {
    pub CustomRenderElements<R> where
        R: ImportAll + ImportMem;
    Pointer=PointerRenderElement<R>,
    Surface=WaylandSurfaceRenderElement<R>,
    #[cfg(feature = "debug")]
    // Note: We would like to borrow this element instead, but that would introduce
    // a feature-dependent lifetime, which introduces a lot more feature bounds
    // as the whole type changes and we can't have an unused lifetime (for when "debug" is disabled)
    // in the declaration.
    Fps=FpsElement<R::TextureId>,
}

impl<R: Renderer> std::fmt::Debug for CustomRenderElements<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pointer(arg0) => f.debug_tuple("Pointer").field(arg0).finish(),
            Self::Surface(arg0) => f.debug_tuple("Surface").field(arg0).finish(),
            #[cfg(feature = "debug")]
            Self::Fps(arg0) => f.debug_tuple("Fps").field(arg0).finish(),
            Self::_GenericCatcher(arg0) => f.debug_tuple("_GenericCatcher").field(arg0).finish(),
        }
    }
}

smithay::backend::renderer::element::render_elements! {
    pub OutputRenderElements<R, E> where R: ImportAll + ImportMem;
    Space=SpaceRenderElements<R, E>,
    Window=Wrap<E>,
    Layer=WaylandSurfaceRenderElement<R>,
    // Distinct enum types only — a second TextureRenderElement variant
    // would collide with Background's From impl, hence the Relocate
    // (0,0) wrapper.
    LayerShadow=RelocateRenderElement<TextureRenderElement<R::TextureId>>,
    Custom=CustomRenderElements<R>,
    Preview=CropRenderElement<RelocateRenderElement<RescaleRenderElement<WindowRenderElement<R>>>>,
    Background=TextureRenderElement<R::TextureId>,
    Snap=SolidColorRenderElement,
}

impl<R: Renderer + ImportAll + ImportMem, E: RenderElement<R> + std::fmt::Debug> std::fmt::Debug
    for OutputRenderElements<R, E>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Space(arg0) => f.debug_tuple("Space").field(arg0).finish(),
            Self::Window(arg0) => f.debug_tuple("Window").field(arg0).finish(),
            Self::Layer(arg0) => f.debug_tuple("Layer").field(arg0).finish(),
            Self::LayerShadow(arg0) => f.debug_tuple("LayerShadow").field(arg0).finish(),
            Self::Custom(arg0) => f.debug_tuple("Custom").field(arg0).finish(),
            Self::Preview(arg0) => f.debug_tuple("Preview").field(arg0).finish(),
            Self::Background(arg0) => f.debug_tuple("Background").field(arg0).finish(),
            Self::Snap(arg0) => f.debug_tuple("Snap").field(arg0).finish(),
            Self::_GenericCatcher(arg0) => f.debug_tuple("_GenericCatcher").field(arg0).finish(),
        }
    }
}

pub fn space_preview_elements<'a, R, C>(
    renderer: &'a mut R,
    space: &'a Space<WindowElement>,
    output: &'a Output,
) -> impl Iterator<Item = C> + 'a
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
    C: From<CropRenderElement<RelocateRenderElement<RescaleRenderElement<WindowRenderElement<R>>>>>
        + 'a,
{
    let constrain_behavior = ConstrainBehavior {
        reference: ConstrainReference::BoundingBox,
        behavior: ConstrainScaleBehavior::Fit,
        align: ConstrainAlign::CENTER,
    };

    let preview_padding = 10;

    let elements_on_space = space.elements_for_output(output).count();
    let output_scale = output.current_scale().fractional_scale();
    let output_transform = output.current_transform();
    let output_size = output
        .current_mode()
        .map(|mode| {
            output_transform
                .transform_size(mode.size)
                .to_f64()
                .to_logical(output_scale)
        })
        .unwrap_or_default();

    let max_elements_per_row = 4;
    let elements_per_row = usize::min(elements_on_space, max_elements_per_row);
    let rows = f64::ceil(elements_on_space as f64 / elements_per_row as f64);

    let preview_size = Size::from((
        f64::round(output_size.w / elements_per_row as f64) as i32 - preview_padding * 2,
        f64::round(output_size.h / rows) as i32 - preview_padding * 2,
    ));

    space
        .elements_for_output(output)
        .enumerate()
        .flat_map(move |(element_index, window)| {
            let column = element_index % elements_per_row;
            let row = element_index / elements_per_row;
            let preview_location = Point::from((
                preview_padding + (preview_padding + preview_size.w) * column as i32,
                preview_padding + (preview_padding + preview_size.h) * row as i32,
            ));
            let constrain = Rectangle::new(preview_location, preview_size);
            constrain_space_element(
                renderer,
                window,
                preview_location,
                1.0,
                output_scale,
                constrain,
                constrain_behavior,
            )
        })
}

#[profiling::function]
pub fn output_elements<R>(
    output: &Output,
    space: &Space<WindowElement>,
    custom_elements: impl IntoIterator<Item = CustomRenderElements<R>>,
    renderer: &mut R,
    show_window_preview: bool,
    snap_preview: Option<Rectangle<i32, Logical>>,
) -> (
    Vec<OutputRenderElements<R, WindowRenderElement<R>>>,
    Color32F,
)
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    if let Some(window) = output
        .user_data()
        .get::<FullscreenSurface>()
        .and_then(|f| f.get())
    {
        let scale = output.current_scale().fractional_scale().into();
        let window_render_elements: Vec<WindowRenderElement<R>> =
            AsRenderElements::<R>::render_elements(&window, renderer, (0, 0).into(), scale, 1.0);

        let elements = custom_elements
            .into_iter()
            .map(OutputRenderElements::from)
            .chain(
                window_render_elements
                    .into_iter()
                    .map(|e| OutputRenderElements::Window(Wrap::from(e))),
            )
            .collect::<Vec<_>>();
        (elements, CLEAR_COLOR_FULLSCREEN)
    } else {
        // A layer surface whose destroy raced the unmap path keeps emitting
        // stale frames (a dead launcher was observed painting ~95s after its
        // `destroyed` log). Purge dead layers unconditionally before they are
        // enumerated — a removed layer can never paint.
        {
            let mut map = layer_map_for_output(output);
            let dead: Vec<_> = map
                .layers()
                .filter(|l| !l.layer_surface().alive())
                .cloned()
                .collect();
            for layer in dead {
                tracing::warn!("cosmos: purged dead layer surface from render map");
                map.unmap_layer(&layer);
            }
        }

        let mut output_render_elements = custom_elements
            .into_iter()
            .map(OutputRenderElements::from)
            .collect::<Vec<_>>();

        if show_window_preview && space.elements_for_output(output).count() > 0 {
            output_render_elements.extend(space_preview_elements(renderer, space, output));
        }

        // Mirrors smithay's space_render_elements ordering (upper layers
        // → space windows → lower layers, front-to-back) but interleaves
        // a soft shadow behind each floating card surface — the upstream
        // helper can't inject per-surface decals.
        let output_scale = output.current_scale().fractional_scale();
        let layer_map = layer_map_for_output(output);
        let (lower, upper): (Vec<_>, Vec<_>) = layer_map
            .layers()
            .rev()
            .partition(|s| matches!(s.layer(), WlrLayer::Background | WlrLayer::Bottom));

        for surface in upper {
            let Some(geo) = layer_map.layer_geometry(surface) else {
                continue;
            };
            output_render_elements.extend(
                AsRenderElements::<R>::render_elements::<WaylandSurfaceRenderElement<R>>(
                    surface,
                    renderer,
                    geo.loc.to_physical_precise_round(output_scale),
                    Scale::from(output_scale),
                    1.0,
                )
                .into_iter()
                .map(OutputRenderElements::Layer),
            );
            layer_shadow_elements(
                renderer,
                surface,
                geo,
                output_scale,
                &mut output_render_elements,
            );
        }

        if let Some(output_geo) = space.output_geometry(output) {
            output_render_elements.extend(
                space
                    .render_elements_for_region(renderer, &output_geo, output_scale, 1.0)
                    .into_iter()
                    .map(|e| OutputRenderElements::Window(Wrap::from(e))),
            );
        }

        for surface in lower {
            let Some(geo) = layer_map.layer_geometry(surface) else {
                continue;
            };
            output_render_elements.extend(
                AsRenderElements::<R>::render_elements::<WaylandSurfaceRenderElement<R>>(
                    surface,
                    renderer,
                    geo.loc.to_physical_precise_round(output_scale),
                    Scale::from(output_scale),
                    1.0,
                )
                .into_iter()
                .map(OutputRenderElements::Layer),
            );
        }
        drop(layer_map);

        // Drag-to-edge drop target: under every window, above the
        // wallpaper — the translucent zone a release would snap into.
        if let Some(rect) = snap_preview {
            output_render_elements.extend(snap_preview_elements(renderer, output, rect));
        }

        // Elements render back-to-front: last pushed = bottom-most. The desktop
        // gradient sits under every window/layer surface.
        output_render_elements.extend(background_element(renderer, output));

        (output_render_elements, clear_color())
    }
}

/// Layer surfaces that float as cards over the desktop — everything
/// else (the menubar, fullscreen overlays like the launcher/assist)
/// gets no compositor shadow. The returned px top inset trims surfaces
/// that keep a transparent overhang band above the visible card (the
/// dock's magnification room).
fn layer_shadow_top_inset(namespace: &str) -> Option<i32> {
    match namespace {
        // 24 = shell's dock::MAG_ROOM — the transparent overhang above
        // the dock card must not silhouette into the shadow rect.
        "cosmos-dock" => Some(24),
        "cosmos-quick" | "cosmos-notify" | "cosmos-switcher" => Some(0),
        _ => None,
    }
}

/// A soft drop shadow behind a floating card layer surface (macOS
/// popover/menubar idiom). Reuses the window-shadow SDF pixmap, cached
/// per surface so the repaint only runs when the card resizes.
fn layer_shadow_elements<R>(
    renderer: &mut R,
    surface: &smithay::desktop::LayerSurface,
    geo: Rectangle<i32, Logical>,
    output_scale: f64,
    out: &mut Vec<OutputRenderElements<R, WindowRenderElement<R>>>,
) where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    let Some(top_inset) = layer_shadow_top_inset(surface.namespace()) else {
        return;
    };
    if geo.size.h <= top_inset {
        return;
    }
    let (w, h) = (geo.size.w, geo.size.h - top_inset);
    let key = surface.wl_surface().id().protocol_id();
    LAYER_SHADOWS.with(|cache| {
        let mut cache = cache.borrow_mut();
        // Dead surfaces' entries die with their ids; cap the map so a
        // long session can't accumulate stale pixmaps.
        if cache.len() >= 32 {
            cache.retain(|k, _| *k == key);
        }
        let shadow = cache.entry(key).or_insert_with(ssd::WindowShadow::default);
        shadow.repaint(w, h);
        let origin = (geo.loc + Point::from((0, top_inset)))
            .to_physical_precise_round(output_scale)
            - Point::from((ssd::SHADOW_MARGIN, ssd::SHADOW_MARGIN));
        out.extend(
            AsRenderElements::<R>::render_elements::<TextureRenderElement<R::TextureId>>(
                shadow,
                renderer,
                origin,
                Scale::from(output_scale),
                1.0,
            )
            .into_iter()
            .map(|el| {
                OutputRenderElements::LayerShadow(RelocateRenderElement::from_element(
                    el,
                    (0, 0),
                    Relocate::Relative,
                ))
            }),
        );
    });
}

/// Drop-target outline for drag-to-edge snapping — four opaque hairline
/// rects forming the zone a release would snap into. Solid fills can't
/// alpha-blend on llvmpipe (the 15%-alpha fill painted opaque), and a
/// thin outline reads cleaner than a translucent slab anyway.
fn snap_preview_elements<R>(
    _renderer: &mut R,
    output: &Output,
    rect: Rectangle<i32, Logical>,
) -> Vec<OutputRenderElements<R, WindowRenderElement<R>>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    let scale = output.current_scale().fractional_scale();
    let theme = crate::shell::ssd::current_theme();
    let color = if theme.dark {
        Color32F::new(0.70, 0.71, 0.74, 1.0)
    } else {
        Color32F::new(0.32, 0.32, 0.35, 1.0)
    };
    let geo = rect.to_physical_precise_round(scale);
    let (x, y, w, h) = (geo.loc.x, geo.loc.y, geo.size.w, geo.size.h);
    // ~2 logical px, always an even physical thickness for crispness.
    let t = (2.0 * scale).round().max(2.0) as i32;
    if w <= 2 * t || h <= 2 * t {
        return Vec::new();
    }
    let edges = [
        Rectangle::new((x, y).into(), (w, t).into()),
        Rectangle::new((x, y + h - t).into(), (w, t).into()),
        Rectangle::new((x, y + t).into(), (t, h - 2 * t).into()),
        Rectangle::new((x + w - t, y + t).into(), (t, h - 2 * t).into()),
    ];
    edges
        .into_iter()
        .map(|edge| {
            OutputRenderElements::Snap(SolidColorRenderElement::new(
                Id::new(),
                edge,
                CommitCounter::default(),
                color,
                Kind::Unspecified,
            ))
        })
        .collect()
}

/// Desktop background — a procedural "aurora" wallpaper rendered into a
/// 128x128 texture stretched bilinear to the output: a deep navy base
/// with two soft gaussian colour blooms (indigo + teal) and a gentle
/// corner vignette. Light mode swaps to a pale slate with pastel blooms.
/// No image asset — pure math, rebuilt each frame so the theme toggles
/// apply instantly.
fn background_element<R>(
    renderer: &mut R,
    output: &Output,
) -> Option<OutputRenderElements<R, WindowRenderElement<R>>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    const N: usize = 128;
    // Peak corner darkening — subtle enough to read as depth, not a filter.
    const VIGNETTE: f32 = 0.10;
    let scale = output.current_scale().fractional_scale();
    // Element geometry is in physical pixels: the transformed mode size.
    let size = output
        .current_mode()
        .map(|m| output.current_transform().transform_size(m.size))?;
    let theme = crate::shell::ssd::current_theme();
    // (base rgb, blooms: (x, y, sigma, strength, rgb))
    let (base, blooms): ([f32; 3], [(f32, f32, f32, f32, [f32; 3]); 3]) = if theme.dark {
        (
            [16.0 / 255.0, 20.0 / 255.0, 31.0 / 255.0], // deep navy
            [
                (0.30, 0.26, 0.30, 0.55, [0.21, 0.25, 0.55]), // indigo bloom
                (0.78, 0.70, 0.28, 0.40, [0.09, 0.31, 0.38]), // teal bloom
                (0.62, 0.15, 0.22, 0.30, [0.24, 0.18, 0.40]), // violet whisper
            ],
        )
    } else {
        (
            [0.918, 0.929, 0.957], // pale slate
            [
                (0.30, 0.26, 0.30, 0.45, [0.66, 0.74, 0.94]), // pastel blue
                (0.78, 0.70, 0.28, 0.35, [0.62, 0.86, 0.88]), // pastel teal
                (0.62, 0.15, 0.22, 0.28, [0.76, 0.68, 0.90]), // pastel violet
            ],
        )
    };
    let mut pixels = Vec::with_capacity(N * N * 4);
    for py in 0..N {
        for px in 0..N {
            let u = (px as f32 + 0.5) / N as f32;
            let v = (py as f32 + 0.5) / N as f32;
            // Sum gaussian blooms onto the base colour (per channel).
            let mut c = base;
            for (bx, by, sigma, strength, bc) in blooms {
                let dx = u - bx;
                let dy = v - by;
                let g = (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp() * strength;
                for ch in 0..3 {
                    c[ch] += (bc[ch] - c[ch]) * g.min(1.0);
                }
            }
            // Vignette: smoothstep past 35% radius, easing dark to corners.
            let nx = u - 0.5;
            let ny = v - 0.5;
            let r = ((nx * nx + ny * ny).sqrt() * std::f32::consts::SQRT_2).min(1.0);
            let t = ((r - 0.35) / 0.65).clamp(0.0, 1.0);
            let dim = 1.0 - t * t * (3.0 - 2.0 * t) * VIGNETTE;
            let f = |x: f32| (x * dim * 255.0).clamp(0.0, 255.0) as u8;
            pixels.extend_from_slice(&[f(c[0]), f(c[1]), f(c[2]), 255]);
        }
    }
    // Content depends only on theme + output size — cache the texture
    // instead of re-uploading the gradient every frame.
    let key = ssd::decal_key(3, (theme.dark, size.w, size.h));
    let loc = output.current_location().to_f64().to_physical(scale);
    ssd::decal_element(
        renderer,
        key,
        &pixels,
        N as i32,
        N as i32,
        loc.to_i32_round(),
        1.0,
        Some(size.to_f64().to_logical(scale).to_i32_round()),
    )
    .map(OutputRenderElements::Background)
}

#[allow(clippy::too_many_arguments)]
pub fn render_output<'a, 'd, R>(
    output: &'a Output,
    space: &'a Space<WindowElement>,
    custom_elements: impl IntoIterator<Item = CustomRenderElements<R>>,
    renderer: &'a mut R,
    framebuffer: &'a mut R::Framebuffer<'_>,
    damage_tracker: &'d mut OutputDamageTracker,
    age: usize,
    show_window_preview: bool,
    snap_preview: Option<Rectangle<i32, Logical>>,
) -> Result<RenderOutputResult<'d>, OutputDamageTrackerError<R::Error>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    let (elements, clear_color) = output_elements(
        output,
        space,
        custom_elements,
        renderer,
        show_window_preview,
        snap_preview,
    );
    damage_tracker.render_output(renderer, framebuffer, age, &elements, clear_color)
}

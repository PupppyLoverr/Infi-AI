use std::time::Instant;

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
            constrain_space_element, ConstrainBehavior, ConstrainReference, Space, SpaceElement,
            SpaceRenderElements,
        },
    },
    output::Output,
    reexports::wayland_server::Resource as _,
    utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform},
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
    // Transition-animated elements: rescale-about-origin inside a
    // relative relocate covers zoom-in, shrink-out and slide motion with
    // a single wrapper type per element class.
    WindowAnim=RelocateRenderElement<RescaleRenderElement<WindowRenderElement<R>>>,
    LayerAnim=RelocateRenderElement<RescaleRenderElement<WaylandSurfaceRenderElement<R>>>,
    LayerShadowAnim=RelocateRenderElement<
        RescaleRenderElement<RelocateRenderElement<TextureRenderElement<R::TextureId>>>,
    >,
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
            Self::WindowAnim(arg0) => f.debug_tuple("WindowAnim").field(arg0).finish(),
            Self::LayerAnim(arg0) => f.debug_tuple("LayerAnim").field(arg0).finish(),
            Self::LayerShadowAnim(arg0) => f.debug_tuple("LayerShadowAnim").field(arg0).finish(),
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
    cosmos: &crate::cosmos::CosmosState,
    now: Instant,
) -> (
    Vec<OutputRenderElements<R, WindowRenderElement<R>>>,
    Color32F,
)
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    // ext-session-lock: while locked only the wallpaper and the lock
    // surfaces composite — no window or layer content may leak through.
    if cosmos.session_locked {
        let scale: smithay::utils::Scale<f64> = output.current_scale().fractional_scale().into();
        // Elements composite front-to-back: lock surfaces first, the
        // wallpaper last — pushing it first hid the whole card behind it.
        let mut elements = Vec::new();
        for (surface, target) in &cosmos.lock_surfaces {
            if target != output {
                continue;
            }
            for el in
                smithay::backend::renderer::element::surface::render_elements_from_surface_tree::<
                    R,
                    WaylandSurfaceRenderElement<R>,
                >(
                    renderer,
                    surface.wl_surface(),
                    smithay::utils::Point::<i32, smithay::utils::Physical>::from((0, 0)),
                    scale,
                    1.0,
                    smithay::backend::renderer::element::Kind::Unspecified,
                )
            {
                elements.push(OutputRenderElements::Layer(el));
            }
        }
        elements.extend(background_element(renderer, output));
        return (elements, clear_color());
    }
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
            let anim = layer_anim_tx(cosmos, surface, geo, now);
            output_render_elements.extend(animated_layer_elements(
                surface,
                renderer,
                geo,
                output_scale,
                anim,
            ));
            if !cosmos.config.lite_mode {
                layer_shadow_elements(
                    renderer,
                    surface,
                    geo,
                    output_scale,
                    anim,
                    &mut output_render_elements,
                );
            }
        }

        // Space windows, emitted manually (same ordering as smithay's
        // `render_elements_for_region` — front to back over the output
        // region) so each window's elements can carry its transition
        // transform: map-in zoom+fade, minimize shrink-out, workspace
        // slide. Non-animating windows keep the plain `Window` variant.
        if let Some(output_geo) = space.output_geometry(output) {
            for window in space.elements().rev() {
                let Some(bbox) = space.element_bbox(window) else {
                    continue;
                };
                if !output_geo.overlaps(bbox) {
                    continue;
                }
                let render_loc = space.element_location(window).unwrap_or_default()
                    - window.geometry().loc
                    - output_geo.loc;
                let phys_loc: Point<i32, Physical> =
                    render_loc.to_physical_precise_round(output_scale);
                let id = cosmos.id_of(window);
                let (alpha, wscale) = id
                    .and_then(|id| cosmos.anims.window_tx(id, now))
                    .unwrap_or((1.0, 1.0));
                let dx = id
                    .map(|id| cosmos.anims.ws_dx(id, now, output_geo.size.w))
                    .unwrap_or(0);
                let animating = wscale != 1.0 || dx != 0;
                for el in AsRenderElements::<R>::render_elements::<WindowRenderElement<R>>(
                    window,
                    renderer,
                    phys_loc,
                    Scale::from(output_scale),
                    alpha,
                ) {
                    if animating {
                        let phys_bbox: Rectangle<i32, Physical> = Rectangle::new(
                            (bbox.loc - output_geo.loc).to_physical_precise_round(output_scale),
                            bbox.size.to_f64().to_physical(output_scale).to_i32_round(),
                        );
                        let center = Point::from((
                            phys_bbox.loc.x + phys_bbox.size.w / 2,
                            phys_bbox.loc.y + phys_bbox.size.h / 2,
                        ));
                        let scaled = RescaleRenderElement::from_element(el, center, wscale);
                        output_render_elements.push(OutputRenderElements::WindowAnim(
                            RelocateRenderElement::from_element(
                                scaled,
                                ((dx as f64 * output_scale).round() as i32, 0),
                                Relocate::Relative,
                            ),
                        ));
                    } else {
                        output_render_elements.push(OutputRenderElements::Window(Wrap::from(el)));
                    }
                }
            }
        }

        for surface in lower {
            let Some(geo) = layer_map.layer_geometry(surface) else {
                continue;
            };
            let anim = layer_anim_tx(cosmos, surface, geo, now);
            output_render_elements.extend(animated_layer_elements(
                surface,
                renderer,
                geo,
                output_scale,
                anim,
            ));
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
        // The dock is excluded: its layer surface spans the full screen
        // edge (pill + transparent overhang), so a surface-sized shadow
        // painted a dark strip there. The shell shadows the pill itself.
        "cosmos-quick" | "cosmos-notify" | "cosmos-switcher" => Some(0),
        _ => None,
    }
}

/// Animation transform for a layer surface at `now`:
/// (alpha, dx, dy, scale) — `None` when it isn't animating.
fn layer_anim_tx(
    cosmos: &crate::cosmos::CosmosState,
    surface: &smithay::desktop::LayerSurface,
    geo: Rectangle<i32, Logical>,
    now: Instant,
) -> Option<(f32, i32, i32, f64)> {
    let pid = surface.wl_surface().id().protocol_id();
    cosmos.anims.layer_tx(pid, now, geo.size.w)
}

/// Emit a layer surface's elements, wrapping them in the
/// rescale+relocate transform when its entrance animation is live.
fn animated_layer_elements<R>(
    surface: &smithay::desktop::LayerSurface,
    renderer: &mut R,
    geo: Rectangle<i32, Logical>,
    output_scale: f64,
    anim: Option<(f32, i32, i32, f64)>,
) -> Vec<OutputRenderElements<R, WindowRenderElement<R>>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    let (alpha, dx, dy, scale) = anim.unwrap_or((1.0, 0, 0, 1.0));
    let animating = anim.is_some() && (dx != 0 || dy != 0 || scale != 1.0 || alpha < 1.0);
    let phys_loc = geo.loc.to_physical_precise_round(output_scale);
    AsRenderElements::<R>::render_elements::<WaylandSurfaceRenderElement<R>>(
        surface,
        renderer,
        phys_loc,
        Scale::from(output_scale),
        alpha,
    )
    .into_iter()
    .map(|el| {
        if animating {
            let phys_geo: Rectangle<i32, Physical> = geo.to_physical_precise_round(output_scale);
            let center = Point::from((
                phys_geo.loc.x + phys_geo.size.w / 2,
                phys_geo.loc.y + phys_geo.size.h / 2,
            ));
            OutputRenderElements::LayerAnim(RelocateRenderElement::from_element(
                RescaleRenderElement::from_element(el, center, scale),
                (
                    (dx as f64 * output_scale).round() as i32,
                    (dy as f64 * output_scale).round() as i32,
                ),
                Relocate::Relative,
            ))
        } else {
            OutputRenderElements::Layer(el)
        }
    })
    .collect()
}

/// A soft drop shadow behind a floating card layer surface (macOS
/// popover/menubar idiom). Reuses the window-shadow SDF pixmap, cached
/// per surface so the repaint only runs when the card resizes.
fn layer_shadow_elements<R>(
    renderer: &mut R,
    surface: &smithay::desktop::LayerSurface,
    geo: Rectangle<i32, Logical>,
    output_scale: f64,
    anim: Option<(f32, i32, i32, f64)>,
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
                anim.map(|(a, _, _, _)| a).unwrap_or(1.0),
            )
            .into_iter()
            .map(|el| {
                let relocated = RelocateRenderElement::from_element(el, (0, 0), Relocate::Relative);
                match anim {
                    Some((_, dx, dy, scale)) if dx != 0 || dy != 0 || scale != 1.0 => {
                        let phys_geo: Rectangle<i32, Physical> =
                            geo.to_physical_precise_round(output_scale);
                        let center = Point::from((
                            phys_geo.loc.x + phys_geo.size.w / 2,
                            phys_geo.loc.y + phys_geo.size.h / 2,
                        ));
                        OutputRenderElements::LayerShadowAnim(RelocateRenderElement::from_element(
                            RescaleRenderElement::from_element(relocated, center, scale),
                            (
                                (dx as f64 * output_scale).round() as i32,
                                (dy as f64 * output_scale).round() as i32,
                            ),
                            Relocate::Relative,
                        ))
                    }
                    _ => OutputRenderElements::LayerShadow(relocated),
                }
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
    // Accent-tinted drop zone — the Win11 drop-highlight idiom.
    let color = Color32F::new(theme.accent[0], theme.accent[1], theme.accent[2], 1.0);
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
/// Directory holding the shipped wallpaper set — overridable for dev.
fn wallpaper_dir() -> std::path::PathBuf {
    std::env::var("COSMOS_WALLPAPER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/share/cosmos/wallpapers"))
}

/// Decoded pixels + dims of the current wallpaper. Only the latest
/// name@size is kept: a switch drops the previous full-res copy.
fn wallpaper_pixels(name: &str, w: u32, h: u32) -> Option<(std::sync::Arc<Vec<u8>>, u32, u32)> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<
        Mutex<std::collections::HashMap<String, (std::sync::Arc<Vec<u8>>, u32, u32)>>,
    > = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let key = format!("{name}@{w}x{h}");
    if let Some(hit) = cache.lock().ok()?.get(&key) {
        return Some(hit.clone());
    }
    let dir = wallpaper_dir();
    // Prefer the exact mode size, then 4K, then 1080p.
    let decoded = [
        format!("{name}-{w}x{h}.png"),
        format!("{name}-3840x2160.png"),
        format!("{name}-1920x1080.png"),
    ]
    .iter()
    .find_map(|f| image::open(dir.join(f)).ok())
    .map(|img| img.to_rgba8());
    let img = decoded?;
    // Accent = dominant saturated colour, extracted once per load and
    // published through the accent IPC (`accent = "auto"`).
    if let Some(rgb) = dominant_saturated(&img) {
        cosmos_ipc::set_wallpaper_accent(rgb);
        tracing::info!(?rgb, wallpaper = %name, "cosmos: wallpaper accent extracted");
        // Re-persist config.json so its `accent_rgb` carries the new
        // auto accent to out-of-process uitk apps (mtime → re-theme).
        crate::cosmos::CosmosConfig::load().save();
    }
    let (iw, ih) = (img.width(), img.height());
    let entry = (std::sync::Arc::new(img.into_raw()), iw, ih);
    let mut cache = cache.lock().ok()?;
    cache.clear();
    cache.insert(key, entry.clone());
    drop(cache);
    trim_heap();
    Some(entry)
}

/// The most saturated bucket's average colour — a good "dominant
/// colour" proxy for smooth ambient wallpapers.
fn dominant_saturated(img: &image::RgbaImage) -> Option<[u8; 3]> {
    const BINS: usize = 24;
    let mut sum = [[0f64; 3]; BINS];
    let mut cnt = [0u32; BINS];
    let mut sat_acc = [0f64; BINS];
    // Sample every 16th pixel — plenty for a smooth gradient.
    let px = img.as_raw();
    for i in (0..px.len() / 4).step_by(16) {
        let (r, g, b) = (
            px[i * 4] as f64 / 255.0,
            px[i * 4 + 1] as f64 / 255.0,
            px[i * 4 + 2] as f64 / 255.0,
        );
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        if sat < 0.15 || max < 0.10 {
            continue;
        }
        let hue = if (max - min) < f64::EPSILON {
            0.0
        } else if max == r {
            60.0 * (((g - b) / (max - min)) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / (max - min) + 2.0)
        } else {
            60.0 * ((r - g) / (max - min) + 4.0)
        };
        let bin = (((hue + 360.0) % 360.0) / 360.0 * BINS as f64) as usize % BINS;
        // Weight by saturation × brightness so vivid hues win.
        let wgt = sat * max;
        sum[bin][0] += r * wgt;
        sum[bin][1] += g * wgt;
        sum[bin][2] += b * wgt;
        sat_acc[bin] += wgt;
        cnt[bin] += 1;
    }
    let best = (0..BINS).max_by(|a, b| sat_acc[*a].partial_cmp(&sat_acc[*b]).unwrap())?;
    if cnt[best] == 0 {
        return None;
    }
    // Lift the average toward full saturation — raw averages read muddy
    // as an accent. Scale channels so max ≈ 0.85.
    let m = sat_acc[best].max(f64::EPSILON);
    let mut rgb = [sum[best][0] / m, sum[best][1] / m, sum[best][2] / m];
    let peak = rgb[0].max(rgb[1]).max(rgb[2]).max(0.01);
    for c in &mut rgb {
        *c = (*c / peak * 0.85).clamp(0.12, 1.0);
    }
    Some([
        (rgb[0] * 255.0) as u8,
        (rgb[1] * 255.0) as u8,
        (rgb[2] * 255.0) as u8,
    ])
}

fn background_element<R>(
    renderer: &mut R,
    output: &Output,
) -> Option<OutputRenderElements<R, WindowRenderElement<R>>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    const N: usize = 128;
    let scale = output.current_scale().fractional_scale();
    // Element geometry is in physical pixels: the transformed mode size.
    let size = output
        .current_mode()
        .map(|m| output.current_transform().transform_size(m.size))?;
    // Shipped wallpaper file first — procedural aurora stays as the
    // fallback when no file is installed (dev shells, unit env).
    let name = ssd::wallpaper_name();
    if !name.is_empty() {
        if let Some((px, iw, ih)) = wallpaper_pixels(&name, size.w as u32, size.h as u32) {
            let loc = output.current_location().to_f64().to_physical(scale);
            // Cover-fit: the texture is stretched to the output — the
            // generator's aspect matches common modes closely enough
            // that fill beats letterboxing.
            let key = ssd::decal_key(5, (true, name.len() as u32 ^ size.w as u32, size.w, size.h));
            return ssd::decal_element(
                renderer,
                key,
                &px,
                iw as i32,
                ih as i32,
                loc.to_i32_round(),
                1.0,
                Some(size.to_f64().to_logical(scale).to_i32_round()),
            )
            .map(OutputRenderElements::Background);
        }
    }
    let theme = crate::shell::ssd::current_theme();
    // Base + blooms come from the accent preset (already resolved for
    // dark/light inside the theme). No vignette — flat blooms only.
    let (base, blooms): ([f32; 3], [(f32, f32, f32, f32, [f32; 3]); 3]) =
        (theme.bg_base, theme.blooms);
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
            let f = |x: f32| (x * 255.0).clamp(0.0, 255.0) as u8;
            pixels.extend_from_slice(&[f(c[0]), f(c[1]), f(c[2]), 255]);
        }
    }
    // Content depends on theme + accent + output size — cache the
    // texture instead of re-uploading the gradient every frame. The
    // accent rgb bits stand in for the preset so a theme switch
    // re-renders instead of serving a stale bloom texture.
    let key = ssd::decal_key(3, (theme.dark, theme.accent[0].to_bits(), size.w, size.h));
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
    cosmos: &crate::cosmos::CosmosState,
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
        cosmos,
        Instant::now(),
    );
    damage_tracker.render_output(renderer, framebuffer, age, &elements, clear_color)
}

/// Render one window (its SSD chrome included) offscreen at 1× and
/// write it as a PNG at most `max_w` wide. Backs the Snap Assist tiles.
#[profiling::function]
pub fn capture_window_to_png<R, T>(
    renderer: &mut R,
    window: &crate::shell::WindowElement,
    max_w: u32,
    path: &std::path::Path,
) -> Result<(), String>
where
    R: Renderer
        + ImportAll
        + ImportMem
        + smithay::backend::renderer::Offscreen<T>
        + smithay::backend::renderer::Bind<T>
        + smithay::backend::renderer::ExportMem,
    R::TextureId: Clone + smithay::backend::renderer::Texture + 'static,
{
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::Offscreen;
    use smithay::utils::Buffer as BufferCoord;

    let bbox = SpaceElement::bbox(window);
    if bbox.size.w <= 0 || bbox.size.h <= 0 {
        return Err("window has no size yet".to_string());
    }
    let size: Size<i32, Physical> = (bbox.size.w, bbox.size.h).into();
    let buf_size: Size<i32, BufferCoord> = Size::from((size.w, size.h));
    let mut texture: T = Offscreen::<T>::create_buffer(renderer, Fourcc::Argb8888, buf_size)
        .map_err(|e| format!("offscreen texture alloc failed: {e}"))?;
    let mut fb = renderer
        .bind(&mut texture)
        .map_err(|e| format!("bind offscreen target failed: {e}"))?;
    // bbox.loc is relative to the element origin (negative with SSD
    // chrome), so shifting by it puts the whole bbox at (0, 0).
    let elements: Vec<crate::shell::WindowRenderElement<R>> =
        AsRenderElements::<R>::render_elements(
            window,
            renderer,
            (-bbox.loc.x, -bbox.loc.y).into(),
            Scale::from(1.0),
            1.0,
        );
    let mut tracker = OutputDamageTracker::new(size, 1.0, Transform::Normal);
    tracker
        .render_output(
            renderer,
            &mut fb,
            0,
            &elements,
            Color32F::new(0.0, 0.0, 0.0, 0.0),
        )
        .map_err(|e| format!("offscreen render failed: {e:?}"))?;
    let mapping = renderer
        .copy_framebuffer(&fb, Rectangle::from_size(buf_size), Fourcc::Abgr8888)
        .map_err(|e| format!("framebuffer readback failed: {e}"))?;
    let pixels = renderer
        .map_texture(&mapping)
        .map_err(|e| format!("map readback failed: {e}"))?;
    let img = image::RgbaImage::from_raw(size.w as u32, size.h as u32, pixels.to_vec())
        .ok_or_else(|| "pixel buffer size mismatch".to_string())?;
    let (w, h) = img.dimensions();
    let tw = w.min(max_w.max(1));
    let th = ((h as u64 * tw as u64) / w as u64).max(1) as u32;
    image::imageops::thumbnail(&img, tw, th)
        .save(path)
        .map_err(|e| format!("write {} failed: {e}", path.display()))
}

/// Render one output's full desktop (windows + layer surfaces, no cursor)
/// offscreen and write it to `path` as a PNG. Used by the IPC
/// `Screenshot` request — the xdg-desktop-portal backend turns it into
/// the freedesktop Screenshot interface, and the drive harness uses it
/// for pixel-true guest evidence on any backend (udev or winit).
///
/// Offscreen readback: an offscreen target `T` is bound, the same
/// `output_elements` stack composites into it, and `copy_framebuffer`
/// pulls the pixels back to CPU memory.
#[profiling::function]
pub fn capture_output_to_png<R, T>(
    renderer: &mut R,
    space: &Space<WindowElement>,
    cosmos: &crate::cosmos::CosmosState,
    output: &Output,
    path: &std::path::Path,
) -> Result<(), String>
where
    R: Renderer
        + ImportAll
        + ImportMem
        + smithay::backend::renderer::Offscreen<T>
        + smithay::backend::renderer::Bind<T>
        + smithay::backend::renderer::ExportMem,
    R::TextureId: Clone + 'static,
{
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::Offscreen;
    use smithay::utils::Buffer as BufferCoord;

    let size: Size<i32, Physical> = output
        .current_mode()
        .map(|m| output.current_transform().transform_size(m.size))
        .ok_or_else(|| "output has no current mode".to_string())?;
    let buf_size: Size<i32, BufferCoord> = Size::from((size.w, size.h));

    let mut texture: T = Offscreen::<T>::create_buffer(renderer, Fourcc::Argb8888, buf_size)
        .map_err(|e| format!("offscreen texture alloc failed: {e}"))?;

    let mut fb = renderer
        .bind(&mut texture)
        .map_err(|e| format!("bind offscreen target failed: {e}"))?;

    let (elements, clear_color) = output_elements(
        output,
        space,
        std::iter::empty(),
        renderer,
        false,
        None,
        cosmos,
        Instant::now(),
    );

    let mut tracker = OutputDamageTracker::from_output(output);
    tracker
        .render_output(renderer, &mut fb, 0, &elements, clear_color)
        .map_err(|e| format!("offscreen render failed: {e:?}"))?;

    let region = Rectangle::from_size(buf_size);
    let mapping = renderer
        .copy_framebuffer(&fb, region, Fourcc::Abgr8888)
        .map_err(|e| format!("framebuffer readback failed: {e}"))?;
    let pixels = renderer
        .map_texture(&mapping)
        .map_err(|e| format!("map readback failed: {e}"))?;

    // Abgr8888 little-endian memory order = R,G,B,A bytes.
    image::RgbaImage::from_raw(size.w as u32, size.h as u32, pixels.to_vec())
        .ok_or_else(|| "pixel buffer size mismatch".to_string())?
        .save(path)
        .map_err(|e| format!("write {} failed: {e}", path.display()))
}

/// Hands freed heap pages back to the kernel. glibc keeps freed memory
/// (decoded wallpapers, closed windows' textures) mapped otherwise.
pub(crate) fn trim_heap() {
    #[cfg(target_env = "gnu")]
    // SAFETY: malloc_trim only walks glibc's own allocator state.
    unsafe {
        libc::malloc_trim(0);
    }
}

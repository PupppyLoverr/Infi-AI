use smithay::{
    backend::renderer::{
        damage::{Error as OutputDamageTrackerError, OutputDamageTracker, RenderOutputResult},
        element::{
            surface::WaylandSurfaceRenderElement,
            texture::{TextureBuffer, TextureRenderElement},
            utils::{
                ConstrainAlign, ConstrainScaleBehavior, CropRenderElement, RelocateRenderElement,
                RescaleRenderElement,
            },
            AsRenderElements, Kind, RenderElement, Wrap,
        },
        Color32F, ImportAll, ImportMem, Renderer,
    },
    desktop::space::{
        constrain_space_element, ConstrainBehavior, ConstrainReference, Space, SpaceRenderElements,
    },
    output::Output,
    utils::{Point, Rectangle, Size, Transform},
};

#[cfg(feature = "debug")]
use crate::drawing::FpsElement;
use crate::{
    drawing::{clear_color, PointerRenderElement, CLEAR_COLOR_FULLSCREEN},
    shell::{FullscreenSurface, WindowElement, WindowRenderElement},
};

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
    Custom=CustomRenderElements<R>,
    Preview=CropRenderElement<RelocateRenderElement<RescaleRenderElement<WindowRenderElement<R>>>>,
    Background=TextureRenderElement<R::TextureId>,
}

impl<R: Renderer + ImportAll + ImportMem, E: RenderElement<R> + std::fmt::Debug> std::fmt::Debug
    for OutputRenderElements<R, E>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Space(arg0) => f.debug_tuple("Space").field(arg0).finish(),
            Self::Window(arg0) => f.debug_tuple("Window").field(arg0).finish(),
            Self::Custom(arg0) => f.debug_tuple("Custom").field(arg0).finish(),
            Self::Preview(arg0) => f.debug_tuple("Preview").field(arg0).finish(),
            Self::Background(arg0) => f.debug_tuple("Background").field(arg0).finish(),
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
        let mut output_render_elements = custom_elements
            .into_iter()
            .map(OutputRenderElements::from)
            .collect::<Vec<_>>();

        if show_window_preview && space.elements_for_output(output).count() > 0 {
            output_render_elements.extend(space_preview_elements(renderer, space, output));
        }

        let space_elements = smithay::desktop::space::space_render_elements::<_, WindowElement, _>(
            renderer,
            [space],
            output,
            1.0,
        )
        .expect("output without mode?");
        output_render_elements.extend(space_elements.into_iter().map(OutputRenderElements::Space));

        // Elements render back-to-front: last pushed = bottom-most. The desktop
        // gradient sits under every window/layer surface.
        output_render_elements.extend(background_element(renderer, output));

        (output_render_elements, clear_color())
    }
}

/// Subtle vertical monochrome gradient as the desktop background — a 1x256
/// texture stretched to the output. Rebuilt each frame (1 KiB) so theme
/// changes apply instantly.
fn background_element<R>(
    renderer: &mut R,
    output: &Output,
) -> Option<OutputRenderElements<R, WindowRenderElement<R>>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    let scale = output.current_scale().fractional_scale();
    // Element geometry is in physical pixels: the transformed mode size.
    let size = output
        .current_mode()
        .map(|m| output.current_transform().transform_size(m.size))?;
    let theme = crate::shell::ssd::current_theme();
    let c = theme.background;
    // Bottom rows drift ~2% lighter (dark) / darker (light) — barely visible,
    // keeps the desktop from reading as a dead flat fill.
    let lift = if theme.dark { 0.022 } else { -0.018 };
    let mut pixels = Vec::with_capacity(256 * 4);
    for i in 0..256u32 {
        let k = i as f32 / 255.0;
        let f = |v: f32| ((v + lift * k) * 255.0).clamp(0.0, 255.0) as u8;
        pixels.extend_from_slice(&[f(c[0]), f(c[1]), f(c[2]), 255]);
    }
    let buffer = TextureBuffer::<R::TextureId>::from_memory(
        renderer,
        &pixels,
        smithay::backend::allocator::Fourcc::Abgr8888,
        (1, 256),
        false,
        1,
        Transform::Normal,
        None,
    )
    .ok()?;
    let loc = output.current_location().to_f64().to_physical(scale);
    Some(OutputRenderElements::Background(
        TextureRenderElement::from_texture_buffer(
            loc,
            &buffer,
            None,
            None,
            Some(size.to_f64().to_logical(scale).to_i32_round()),
            Kind::Unspecified,
        ),
    ))
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
    );
    damage_tracker.render_output(renderer, framebuffer, age, &elements, clear_color)
}

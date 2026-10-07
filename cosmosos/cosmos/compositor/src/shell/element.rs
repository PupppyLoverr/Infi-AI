use std::{borrow::Cow, time::Duration};

use smithay::{
    backend::renderer::{
        element::{
            surface::WaylandSurfaceRenderElement, texture::TextureRenderElement, AsRenderElements,
            Element,
        },
        ImportAll, ImportMem, Renderer, Texture,
    },
    desktop::{
        space::SpaceElement, utils::OutputPresentationFeedback, Window, WindowSurface,
        WindowSurfaceType,
    },
    input::{
        pointer::{
            AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent,
            GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
            GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, MotionEvent,
            PointerTarget, RelativeMotionEvent,
        },
        touch::TouchTarget,
        Seat,
    },
    output::Output,
    reexports::{
        wayland_protocols::wp::presentation_time::server::wp_presentation_feedback,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    render_elements,
    utils::{user_data::UserDataMap, IsAlive, Logical, Physical, Point, Rectangle, Scale, Serial},
    wayland::{
        compositor::SurfaceData as WlSurfaceData, dmabuf::DmabufFeedback, seat::WaylandFocus,
    },
};

use crate::{focus::PointerFocusTarget, state::Backend, AnvilState};

#[derive(Debug, Clone, PartialEq)]
pub struct WindowElement(pub Window);

impl WindowElement {
    pub fn surface_under(
        &self,
        location: Point<f64, Logical>,
        window_type: WindowSurfaceType,
    ) -> Option<(PointerFocusTarget, Point<i32, Logical>)> {
        let tb = self.titlebar_height();
        if tb > 0 && location.y < tb as f64 {
            return Some((PointerFocusTarget::SSD(SSD(self.clone())), Point::default()));
        }
        let offset = Point::from((0, tb));

        let surface_under = self
            .0
            .surface_under(location - offset.to_f64(), window_type);
        let (under, loc) = match self.0.underlying_surface() {
            WindowSurface::Wayland(_) => {
                surface_under.map(|(surface, loc)| (PointerFocusTarget::WlSurface(surface), loc))
            }
            #[cfg(feature = "xwayland")]
            WindowSurface::X11(s) => {
                surface_under.map(|(_, loc)| (PointerFocusTarget::X11Surface(s.clone()), loc))
            }
        }?;
        Some((under, loc + offset))
    }

    pub fn with_surfaces<F>(&self, processor: F)
    where
        F: FnMut(&WlSurface, &WlSurfaceData),
    {
        self.0.with_surfaces(processor);
    }

    pub fn send_frame<T, F>(
        &self,
        output: &Output,
        time: T,
        throttle: Option<Duration>,
        primary_scan_out_output: F,
    ) where
        T: Into<Duration>,
        F: FnMut(&WlSurface, &WlSurfaceData) -> Option<Output> + Copy,
    {
        self.0
            .send_frame(output, time, throttle, primary_scan_out_output)
    }

    pub fn send_dmabuf_feedback<'a, P, F>(
        &self,
        output: &Output,
        primary_scan_out_output: P,
        select_dmabuf_feedback: F,
    ) where
        P: FnMut(&WlSurface, &WlSurfaceData) -> Option<Output> + Copy,
        F: Fn(&WlSurface, &WlSurfaceData) -> &'a DmabufFeedback + Copy,
    {
        self.0
            .send_dmabuf_feedback(output, primary_scan_out_output, select_dmabuf_feedback)
    }

    pub fn take_presentation_feedback<F1, F2>(
        &self,
        output_feedback: &mut OutputPresentationFeedback,
        primary_scan_out_output: F1,
        presentation_feedback_flags: F2,
    ) where
        F1: FnMut(&WlSurface, &WlSurfaceData) -> Option<Output> + Copy,
        F2: FnMut(&WlSurface, &WlSurfaceData) -> wp_presentation_feedback::Kind + Copy,
    {
        self.0.take_presentation_feedback(
            output_feedback,
            primary_scan_out_output,
            presentation_feedback_flags,
        )
    }

    #[cfg(feature = "xwayland")]
    #[inline]
    pub fn is_x11(&self) -> bool {
        self.0.is_x11()
    }

    #[inline]
    pub fn is_wayland(&self) -> bool {
        self.0.is_wayland()
    }

    #[inline]
    pub fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        self.0.wl_surface()
    }

    #[inline]
    pub fn user_data(&self) -> &UserDataMap {
        self.0.user_data()
    }
}

impl IsAlive for WindowElement {
    #[inline]
    fn alive(&self) -> bool {
        self.0.alive()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SSD(WindowElement);

impl IsAlive for SSD {
    #[inline]
    fn alive(&self) -> bool {
        self.0.alive()
    }
}

impl WaylandFocus for SSD {
    #[inline]
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        self.0.wl_surface()
    }
}

impl<BackendData: Backend> PointerTarget<AnvilState<BackendData>> for SSD {
    fn enter(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        event: &MotionEvent,
    ) {
        let mut state = self.0.decoration_state();
        if state.is_ssd {
            state.header_bar.pointer_enter(event.location);
        }
    }
    fn motion(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        event: &MotionEvent,
    ) {
        let mut state = self.0.decoration_state();
        if state.is_ssd {
            state.header_bar.pointer_enter(event.location);
        }
    }
    fn relative_motion(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &RelativeMotionEvent,
    ) {
    }
    fn button(
        &self,
        seat: &Seat<AnvilState<BackendData>>,
        data: &mut AnvilState<BackendData>,
        event: &ButtonEvent,
    ) {
        // ButtonEvent fires for press AND release — dispatching both
        // double-fires every disc (and both toggle maximize).
        if !matches!(event.state, smithay::backend::input::ButtonState::Pressed) {
            return;
        }
        let (is_ssd, zone) = {
            let mut state = self.0.decoration_state();
            let (is_ssd, zone) = (state.is_ssd, state.header_bar.zone_at_pointer());
            // A second quick press on the non-button band is a
            // double-click → maximize/restore (zone 2), not a move grab.
            let zone = match (is_ssd, zone, state.header_bar.pointer_loc) {
                (true, None, Some(loc)) if state.register_title_press(loc) => Some(2),
                (_, z, _) => z,
            };
            (is_ssd, zone)
        };
        if is_ssd {
            // Dispatch only after the WindowState borrow is dropped — the
            // maximize/move paths re-read decoration_state.
            super::ssd::HeaderBar::dispatch_click(zone, seat, data, &self.0, event.serial);
        }
    }
    fn axis(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _frame: AxisFrame,
    ) {
    }
    fn frame(&self, _seat: &Seat<AnvilState<BackendData>>, _data: &mut AnvilState<BackendData>) {}
    fn leave(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _serial: Serial,
        _time: u32,
    ) {
        let mut state = self.0.decoration_state();
        if state.is_ssd {
            state.header_bar.pointer_leave();
        }
    }
    fn gesture_swipe_begin(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GestureSwipeBeginEvent,
    ) {
    }
    fn gesture_swipe_update(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GestureSwipeUpdateEvent,
    ) {
    }
    fn gesture_swipe_end(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GestureSwipeEndEvent,
    ) {
    }
    fn gesture_pinch_begin(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GesturePinchBeginEvent,
    ) {
    }
    fn gesture_pinch_update(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GesturePinchUpdateEvent,
    ) {
    }
    fn gesture_pinch_end(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GesturePinchEndEvent,
    ) {
    }
    fn gesture_hold_begin(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GestureHoldBeginEvent,
    ) {
    }
    fn gesture_hold_end(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &GestureHoldEndEvent,
    ) {
    }
}

impl<BackendData: Backend> TouchTarget<AnvilState<BackendData>> for SSD {
    fn down(
        &self,
        seat: &Seat<AnvilState<BackendData>>,
        data: &mut AnvilState<BackendData>,
        event: &smithay::input::touch::DownEvent,
        _seq: Serial,
    ) {
        let mut state = self.0.decoration_state();
        if state.is_ssd {
            state.header_bar.pointer_enter(event.location);
            state
                .header_bar
                .touch_down(seat, data, &self.0, event.serial);
        }
    }

    fn up(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        data: &mut AnvilState<BackendData>,
        _event: &smithay::input::touch::UpEvent,
        _seq: Serial,
    ) {
        let (is_ssd, zone) = {
            let state = self.0.decoration_state();
            (state.is_ssd, state.header_bar.zone_at_pointer())
        };
        if is_ssd {
            super::ssd::HeaderBar::dispatch_touch_up(zone, data, &self.0);
        }
    }

    fn motion(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        event: &smithay::input::touch::MotionEvent,
        _seq: Serial,
    ) {
        let mut state = self.0.decoration_state();
        if state.is_ssd {
            state.header_bar.pointer_enter(event.location);
        }
    }

    fn frame(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _seq: Serial,
    ) {
    }

    fn cancel(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _seq: Serial,
    ) {
    }

    fn shape(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &smithay::input::touch::ShapeEvent,
        _seq: Serial,
    ) {
    }

    fn orientation(
        &self,
        _seat: &Seat<AnvilState<BackendData>>,
        _data: &mut AnvilState<BackendData>,
        _event: &smithay::input::touch::OrientationEvent,
        _seq: Serial,
    ) {
    }
}

impl SpaceElement for WindowElement {
    fn geometry(&self) -> Rectangle<i32, Logical> {
        let mut geo = SpaceElement::geometry(&self.0);
        geo.size.h += self.titlebar_height();
        geo
    }
    fn bbox(&self) -> Rectangle<i32, Logical> {
        let mut bbox = SpaceElement::bbox(&self.0);
        bbox.size.h += self.titlebar_height();
        bbox
    }
    fn is_in_input_region(&self, point: &Point<f64, Logical>) -> bool {
        let tb = self.titlebar_height() as f64;
        (tb > 0.0 && point.y < tb)
            || SpaceElement::is_in_input_region(&self.0, &(*point - Point::from((0.0, tb))))
    }
    fn z_index(&self) -> u8 {
        SpaceElement::z_index(&self.0)
    }

    fn set_activate(&self, activated: bool) {
        SpaceElement::set_activate(&self.0, activated);
    }
    fn output_enter(&self, output: &Output, overlap: Rectangle<i32, Logical>) {
        SpaceElement::output_enter(&self.0, output, overlap);
    }
    fn output_leave(&self, output: &Output) {
        SpaceElement::output_leave(&self.0, output);
    }
    #[profiling::function]
    fn refresh(&self) {
        SpaceElement::refresh(&self.0);
    }
}

render_elements!(
    pub WindowRenderElement<R> where R: ImportAll + ImportMem, R::TextureId: Texture;
    Window=WaylandSurfaceRenderElement<R>,
    Decoration=TextureRenderElement<R::TextureId>,
);

impl<R: Renderer> std::fmt::Debug for WindowRenderElement<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Window(arg0) => f.debug_tuple("Window").field(arg0).finish(),
            Self::Decoration(arg0) => f.debug_tuple("Decoration").field(arg0).finish(),
            Self::_GenericCatcher(arg0) => f.debug_tuple("_GenericCatcher").field(arg0).finish(),
        }
    }
}

impl<R> AsRenderElements<R> for WindowElement
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + Texture + 'static,
{
    type RenderElement = WindowRenderElement<R>;

    fn render_elements<C: From<Self::RenderElement>>(
        &self,
        renderer: &mut R,
        mut location: Point<i32, Physical>,
        scale: Scale<f64>,
        alpha: f32,
    ) -> Vec<C> {
        let window_bbox = SpaceElement::bbox(&self.0);
        let tb = self.titlebar_height();

        // Fall back to the configured size when the window hasn't
        // committed its first buffer yet — gating the SSD on a
        // non-empty bbox means every new window paints chromeless
        // until its first buffer lands.
        let mut ssd_width = if !window_bbox.is_empty() {
            SpaceElement::geometry(&self.0).size.w
        } else {
            self.0
                .toplevel()
                .and_then(|t| t.current_state().size.map(|s| s.w))
                .unwrap_or_default()
        };
        if tb > 0 && ssd_width == 0 {
            let cached = self.decoration_state().last_ssd_width as i32;
            if cached > 0 {
                ssd_width = cached;
            }
        }

        if tb > 0 && ssd_width > 0 {
            let mut state = self.decoration_state();
            let width = ssd_width;
            state.last_ssd_width = width as u32;

            // Cosmos chrome: repaint the titlebar only when its inputs change.
            let (title, focused) = self
                .wl_surface()
                .as_deref()
                .map(|s| {
                    let (title, _) = crate::cosmos::toplevel_title_app(s);
                    let focused = self
                        .0
                        .toplevel()
                        .map(|t| {
                            t.current_state()
                                .states
                                .contains(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated)
                        })
                        .unwrap_or(false);
                    (title.unwrap_or_default(), focused)
                })
                .unwrap_or_default();
            state
                .header_bar
                .repaint(width as u32, &title, focused, super::ssd::current_theme());
            let mut vec = AsRenderElements::<R>::render_elements::<WindowRenderElement<R>>(
                &state.header_bar,
                renderer,
                location,
                scale,
                alpha,
            );

            location.y += (scale.y * tb as f64) as i32;

            let window_elements: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::render_elements(&self.0, renderer, location, scale, alpha);
            if crate::cosmos::element_debug() {
                tracing::debug!(
                    body = window_elements.len(),
                    tb,
                    ssd_width,
                    "cosmos: ssd window elements"
                );
                // Forensics for the transparent-body wedge: when the
                // surface tree emits nothing for a window that HAS a
                // bbox, probe the same RendererSurfaceState gates
                // WaylandSurfaceRenderElement checks — buffer/view/
                // surface_size/texture-for-context — to name which one
                // silently skipped the element.
                for el in &window_elements {
                    tracing::debug!(
                        "cosmos: body element id={:?} alpha={} src={:?} geo={:?} opaque={}",
                        el.id(),
                        el.alpha(),
                        el.src(),
                        el.geometry(scale),
                        el.opaque_regions(scale).len(),
                    );
                }
                // Read the shm the compositor is about to composite — if the
                // attached buffer reports opaque alpha while the screen shows
                // the body transparent, the corruption is post-import
                // (GL-side); if it reads zero-alpha, the shm itself is
                // transparent at compositor-read time (shm visibility race).
                if let Some(s) = self.0.wl_surface().as_deref() {
                    smithay::wayland::compositor::with_states(s, |states| {
                        use smithay::{
                            reexports::wayland_server::Resource,
                            wayland::compositor::{BufferAssignment, SurfaceAttributes},
                        };
                        let mut attrs = states.cached_state.get::<SurfaceAttributes>();
                        if let Some(BufferAssignment::NewBuffer(buf)) =
                            attrs.current().buffer.as_ref()
                        {
                            let buf_id = buf.id();
                            let _ = smithay::wayland::shm::with_buffer_contents(
                                buf,
                                |ptr, len, meta| {
                                    // SAFETY: heuristic debug read only — the
                                    // client may be repainting concurrently; we
                                    // count bytes and never keep the slice.
                                    let data =
                                        unsafe { std::slice::from_raw_parts(ptr, len) };
                                    let opaque =
                                        data.chunks_exact(4).filter(|px| px[3] != 0).count();
                                    tracing::debug!(
                                        "cosmos: body shm {:?} opaque_px={} len={} meta={:?}",
                                        buf_id,
                                        opaque,
                                        len,
                                        meta,
                                    );
                                },
                            );
                        }
                    });
                }
                if window_elements.is_empty() && !window_bbox.is_empty() {
                    if let Some(s) = self.0.wl_surface().as_deref() {
                        smithay::wayland::compositor::with_states(s, |states| {
                            let data = states
                                .data_map
                                .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>(
                                );
                            match data {
                                Some(d) => {
                                    let d = d.lock().unwrap();
                                    let tex =
                                        d.texture(renderer.context_id()).is_some();
                                    tracing::warn!(
                                        "cosmos: empty body — buffer={} view={} surface_size={:?} texture={}",
                                        d.buffer().is_some(),
                                        d.view().is_some(),
                                        d.surface_size(),
                                        tex,
                                    );
                                }
                                None => {
                                    tracing::warn!("cosmos: empty body — no RendererSurfaceState");
                                }
                            }
                        });
                    }
                }
            }
            vec.extend(window_elements.into_iter().map(Into::into));

            // Drop shadow — painted under the whole window (titlebar +
            // body). Pushed last: elements render back-to-front, so it
            // draws beneath the rest of this window's stack. Only when
            // the surface has real geometry — pre-commit windows have a
            // configured titlebar but no body to shadow.
            if !window_bbox.is_empty() {
                let geo = SpaceElement::geometry(&self.0);
                state.shadow.repaint(geo.size.w, geo.size.h + tb);
                // The shadow pixmap covers the window rect (titlebar
                // included) plus SHADOW_MARGIN on every side — step back
                // over the titlebar advance too.
                let shadow_origin = location
                    - Point::from((super::ssd::SHADOW_MARGIN, super::ssd::SHADOW_MARGIN + tb));
                vec.extend(AsRenderElements::<R>::render_elements::<
                    WindowRenderElement<R>,
                >(
                    &state.shadow, renderer, shadow_origin, scale, alpha
                ));
            }

            vec.into_iter().map(C::from).collect()
        } else {
            let body = AsRenderElements::render_elements(&self.0, renderer, location, scale, alpha);
            if crate::cosmos::element_debug() {
                tracing::debug!(
                    emitted = body.len(),
                    tb,
                    ssd_width,
                    ?window_bbox,
                    "cosmos: plain window elements"
                );
            }
            body.into_iter().map(C::from).collect()
        }
    }
}

//! Cosmos server-side decorations: a single painted titlebar with the window
//! title and minimize/maximize/close buttons, drawn into a pixel buffer and
//! uploaded as a texture each frame it changed.
//!
//! Colors come from [`CosmosTheme`] via a thread-local that the compositor
//! updates whenever the config changes.

use std::cell::{Cell, RefCell, RefMut};

use smithay::{
    backend::renderer::{
        element::{
            texture::{TextureBuffer, TextureRenderElement},
            AsRenderElements, Kind,
        },
        ImportMem, Renderer, Texture,
    },
    desktop::WindowSurface,
    input::Seat,
    utils::{Logical, Point, Scale, Serial},
    wayland::shell::xdg::XdgShellHandler,
};

use tiny_skia::{Color, Paint, PathBuilder, PixmapMut, Stroke};

use crate::{cosmos::CosmosTheme, state::Backend, AnvilState};

use super::WindowElement;

pub const HEADER_BAR_HEIGHT: i32 = 36;
const BUTTON_WIDTH: f64 = 46.0;
const TITLE_PAD_LEFT: f32 = 12.0;
const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 16.0;

thread_local! {
    static CURRENT_THEME: Cell<CosmosTheme> = Cell::new(CosmosTheme::from_config(&crate::cosmos::CosmosConfig::default()));
    static FONT_SYSTEM: RefCell<Option<cosmic_text::FontSystem>> = const { RefCell::new(None) };
    static SWASH_CACHE: RefCell<Option<cosmic_text::SwashCache>> = const { RefCell::new(None) };
}

/// Update the global theme used for all titlebars (called on config change).
pub fn set_global_theme(theme: CosmosTheme) {
    CURRENT_THEME.with(|t| t.set(theme));
}

pub(crate) fn current_theme() -> CosmosTheme {
    CURRENT_THEME.with(Cell::get)
}

/// Which titlebar button the x coordinate is over: 0=min, 1=max, 2=close.
fn button_zone(x: f64, width: f64) -> Option<u8> {
    if x >= width - BUTTON_WIDTH {
        Some(2)
    } else if x >= width - BUTTON_WIDTH * 2.0 {
        Some(1)
    } else if x >= width - BUTTON_WIDTH * 3.0 {
        Some(0)
    } else {
        None
    }
}

#[derive(Debug, PartialEq, Clone)]
struct PaintKey {
    width: u32,
    title: String,
    focused: bool,
    dark: bool,
    hover: Option<u8>,
}

pub struct WindowState {
    pub is_ssd: bool,
    pub header_bar: HeaderBar,
}

#[derive(Debug)]
pub struct HeaderBar {
    pub pointer_loc: Option<Point<f64, Logical>>,
    pub width: u32,
    /// Premultiplied RGBA pixels, `width` × HEADER_BAR_HEIGHT.
    pixels: Vec<u8>,
    paint_key: Option<PaintKey>,
}

impl Default for HeaderBar {
    fn default() -> Self {
        Self {
            pointer_loc: None,
            width: 0,
            pixels: Vec::new(),
            paint_key: None,
        }
    }
}

impl HeaderBar {
    pub fn pointer_enter(&mut self, loc: Point<f64, Logical>) {
        self.pointer_loc = Some(loc);
    }

    pub fn pointer_leave(&mut self) {
        self.pointer_loc = None;
    }

    /// Force a repaint on the next render pass (e.g. after a theme change).
    pub fn invalidate(&mut self) {
        self.paint_key = None;
    }

    /// Repaint the titlebar pixmap if width/title/focus/hover/theme changed.
    pub fn repaint(&mut self, width: u32, title: &str, focused: bool, theme: CosmosTheme) {
        let hover = self
            .pointer_loc
            .and_then(|l| button_zone(l.x, width as f64));
        let key = PaintKey {
            width,
            title: title.to_string(),
            focused,
            dark: theme.dark,
            hover,
        };
        if self.paint_key.as_ref() == Some(&key) {
            return;
        }
        self.paint_key = Some(key);
        self.width = width;
        if width == 0 {
            return;
        }

        self.pixels
            .resize(width as usize * HEADER_BAR_HEIGHT as usize * 4, 0);
        let pointer_loc = self.pointer_loc;
        if let Some(mut pixmap) =
            PixmapMut::from_bytes(&mut self.pixels, width, HEADER_BAR_HEIGHT as u32)
        {
            paint_titlebar(&mut pixmap, title, focused, theme, pointer_loc);
        }
    }

    pub fn clicked<BackendData: Backend>(
        &mut self,
        seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        serial: Serial,
    ) {
        let Some(zone) = self
            .pointer_loc
            .and_then(|l| button_zone(l.x, self.width as f64))
        else {
            // Non-button area: drag-move the window.
            match window.0.underlying_surface() {
                WindowSurface::Wayland(w) => {
                    let seat = seat.clone();
                    let toplevel = w.clone();
                    state
                        .handle
                        .insert_idle(move |data| data.move_request_xdg(&toplevel, &seat, serial));
                }
                #[cfg(feature = "xwayland")]
                WindowSurface::X11(w) => {
                    let window = w.clone();
                    state
                        .handle
                        .insert_idle(move |data| data.move_request_x11(&window));
                }
            }
            return;
        };

        match zone {
            // close
            2 => match window.0.underlying_surface() {
                WindowSurface::Wayland(w) => w.send_close(),
                #[cfg(feature = "xwayland")]
                WindowSurface::X11(w) => {
                    let _ = w.close();
                }
            },
            // maximize / restore
            1 => match window.0.underlying_surface() {
                WindowSurface::Wayland(w) => {
                    let maximized = w
                        .current_state()
                        .states
                        .contains(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Maximized);
                    if maximized {
                        state.unmaximize_request(w.clone());
                    } else {
                        state.maximize_request(w.clone());
                    }
                }
                #[cfg(feature = "xwayland")]
                WindowSurface::X11(w) => {
                    let surface = w.clone();
                    state
                        .handle
                        .insert_idle(move |data| data.maximize_request_x11(&surface));
                }
            },
            // minimize
            0 => {
                let window = window.clone();
                state
                    .handle
                    .insert_idle(move |data| data.minimize_window(&window));
            }
            _ => {}
        }
    }

    pub fn touch_down<BackendData: Backend>(
        &mut self,
        seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        serial: Serial,
    ) {
        // Touch down on a non-button area starts a move; buttons act on release.
        if self
            .pointer_loc
            .and_then(|l| button_zone(l.x, self.width as f64))
            .is_none()
        {
            match window.0.underlying_surface() {
                WindowSurface::Wayland(w) => {
                    let seat = seat.clone();
                    let toplevel = w.clone();
                    state
                        .handle
                        .insert_idle(move |data| data.move_request_xdg(&toplevel, &seat, serial));
                }
                #[cfg(feature = "xwayland")]
                WindowSurface::X11(w) => {
                    let window = w.clone();
                    state
                        .handle
                        .insert_idle(move |data| data.move_request_x11(&window));
                }
            };
        }
    }

    pub fn touch_up<BackendData: Backend>(
        &mut self,
        _seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        _serial: Serial,
    ) {
        let Some(zone) = self
            .pointer_loc
            .and_then(|l| button_zone(l.x, self.width as f64))
        else {
            return;
        };
        match zone {
            2 => match window.0.underlying_surface() {
                WindowSurface::Wayland(w) => w.send_close(),
                #[cfg(feature = "xwayland")]
                WindowSurface::X11(w) => {
                    let _ = w.close();
                }
            },
            1 => match window.0.underlying_surface() {
                WindowSurface::Wayland(w) => state.maximize_request(w.clone()),
                #[cfg(feature = "xwayland")]
                WindowSurface::X11(w) => {
                    let surface = w.clone();
                    state
                        .handle
                        .insert_idle(move |data| data.maximize_request_x11(&surface));
                }
            },
            0 => {
                let window = window.clone();
                state
                    .handle
                    .insert_idle(move |data| data.minimize_window(&window));
            }
            _ => {}
        }
    }
}

/// Draw the titlebar into a premultiplied-RGBA tiny-skia pixmap.
fn paint_titlebar(
    pixmap: &mut PixmapMut<'_>,
    title: &str,
    focused: bool,
    theme: CosmosTheme,
    pointer_loc: Option<Point<f64, Logical>>,
) {
    let w = pixmap.width();
    let h = pixmap.height();
    let f = theme.titlebar_fg;
    let fg = Color::from_rgba(f[0], f[1], f[2], f[3]).unwrap();
    let bg = if focused {
        theme.titlebar_bg_focused
    } else {
        theme.titlebar_bg
    };
    pixmap.fill(Color::from_rgba(bg[0], bg[1], bg[2], bg[3]).unwrap());

    // Bottom 1px separator line.
    let sep = if theme.dark {
        Color::from_rgba(1.0, 1.0, 1.0, 0.06).unwrap()
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.10).unwrap()
    };
    pixmap.fill_rect(
        tiny_skia::Rect::from_xywh(0.0, h as f32 - 1.0, w as f32, 1.0).unwrap(),
        &Paint {
            shader: tiny_skia::Shader::SolidColor(sep),
            ..Default::default()
        },
        tiny_skia::Transform::default(),
        None,
    );

    // Button hover fills.
    if let Some(zone) = pointer_loc.and_then(|l| button_zone(l.x, w as f64)) {
        let hover = theme.button_hover;
        let x = w as f32 - BUTTON_WIDTH as f32 * (3 - zone) as f32;
        pixmap.fill_rect(
            tiny_skia::Rect::from_xywh(x, 0.0, BUTTON_WIDTH as f32, h as f32).unwrap(),
            &Paint {
                shader: tiny_skia::Shader::SolidColor(
                    Color::from_rgba(hover[0], hover[1], hover[2], hover[3]).unwrap(),
                ),
                ..Default::default()
            },
            tiny_skia::Transform::default(),
            None,
        );
    }

    // Button glyphs, right to left: close, maximize, minimize.
    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(if focused {
            fg
        } else {
            let mut c = f;
            c[3] *= 0.55;
            Color::from_rgba(c[0], c[1], c[2], c[3]).unwrap()
        }),
        anti_alias: true,
        ..Default::default()
    };
    let stroke = Stroke {
        width: 1.2,
        ..Default::default()
    };

    let h_f = h as f32;
    let cy = h_f / 2.0;
    for zone in 0..3u8 {
        let cx = w as f32 - BUTTON_WIDTH as f32 * (3 - zone) as f32 + BUTTON_WIDTH as f32 / 2.0;
        match zone {
            // close: ✕
            2 => {
                let r = 5.0;
                let mut pb = PathBuilder::new();
                pb.move_to(cx - r, cy - r);
                pb.line_to(cx + r, cy + r);
                pb.move_to(cx + r, cy - r);
                pb.line_to(cx - r, cy + r);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        tiny_skia::Transform::default(),
                        None,
                    );
                }
            }
            // maximize: □
            1 => {
                let r = 5.0;
                let mut pb = PathBuilder::new();
                pb.move_to(cx - r, cy - r);
                pb.line_to(cx + r, cy - r);
                pb.line_to(cx + r, cy + r);
                pb.line_to(cx - r, cy + r);
                pb.close();
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        tiny_skia::Transform::default(),
                        None,
                    );
                }
            }
            // minimize: —
            _ => {
                let r = 5.0;
                let mut pb = PathBuilder::new();
                pb.move_to(cx - r, cy);
                pb.line_to(cx + r, cy);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        tiny_skia::Transform::default(),
                        None,
                    );
                }
            }
        }
    }

    // Title text, left side.
    let text_max_w = w as f32 - BUTTON_WIDTH as f32 * 3.0 - TITLE_PAD_LEFT * 2.0;
    if text_max_w > 8.0 && !title.is_empty() {
        draw_title(pixmap, title, text_max_w, fg, focused);
    }
}

fn draw_title(pixmap: &mut PixmapMut<'_>, title: &str, max_w: f32, fg: Color, focused: bool) {
    FONT_SYSTEM.with(|fs_cell| {
        let mut fs_opt = fs_cell.borrow_mut();
        let fs = fs_opt.get_or_insert_with(cosmic_text::FontSystem::new);
        SWASH_CACHE.with(|cache_cell| {
            let mut cache_opt = cache_cell.borrow_mut();
            let cache = cache_opt.get_or_insert_with(cosmic_text::SwashCache::new);

            let mut buffer =
                cosmic_text::Buffer::new(fs, cosmic_text::Metrics::new(FONT_SIZE, LINE_HEIGHT));
            buffer.set_size(fs, Some(max_w), Some(LINE_HEIGHT));
            buffer.set_text(
                fs,
                title,
                &cosmic_text::Attrs::new().family(cosmic_text::Family::SansSerif),
                cosmic_text::Shaping::Advanced,
            );
            buffer.shape_until_scroll(fs, false);

            let mut color = cosmic_text::Color::rgba(
                (fg.red() * 255.0) as u8,
                (fg.green() * 255.0) as u8,
                (fg.blue() * 255.0) as u8,
                255,
            );
            if !focused {
                color = cosmic_text::Color::rgba(color.r(), color.g(), color.b(), 140);
            }

            let x0 = TITLE_PAD_LEFT as i32;
            let y0 = ((pixmap.height() as f32 - LINE_HEIGHT) / 2.0).max(0.0) as i32;
            let data_w = pixmap.width() as i32;
            let data_h = pixmap.height() as i32;
            let pixels = pixmap.pixels_mut();

            buffer.draw(fs, cache, color, |x, y, _w, _h, c| {
                let px = x0 + x;
                let py = y0 + y;
                if px < 0 || py < 0 || px >= data_w || py >= data_h {
                    return;
                }
                let i = (py * data_w + px) as usize;
                let Some(dst) = pixels.get_mut(i) else { return };
                // Source-over blend of premultiplied color.
                let sa = c.a() as u32;
                let inv = 255 - sa;
                let dr = dst.red() as u32;
                let dg = dst.green() as u32;
                let db = dst.blue() as u32;
                let da = dst.alpha() as u32;
                *dst = tiny_skia::PremultipliedColorU8::from_rgba(
                    ((c.r() as u32 * sa + dr * inv) / 255).min(255) as u8,
                    ((c.g() as u32 * sa + dg * inv) / 255).min(255) as u8,
                    ((c.b() as u32 * sa + db * inv) / 255).min(255) as u8,
                    (sa + da * inv / 255).min(255) as u8,
                )
                .unwrap_or(*dst);
            });
        });
    });
}

impl<R: Renderer> AsRenderElements<R> for HeaderBar
where
    R: ImportMem,
    R::TextureId: Texture + Clone + 'static,
{
    type RenderElement = TextureRenderElement<R::TextureId>;

    fn render_elements<C: From<Self::RenderElement>>(
        &self,
        renderer: &mut R,
        location: Point<i32, smithay::utils::Physical>,
        _scale: Scale<f64>,
        alpha: f32,
    ) -> Vec<C> {
        if self.width == 0 || self.pixels.is_empty() {
            return vec![];
        }
        let Ok(buffer) = TextureBuffer::<R::TextureId>::from_memory(
            renderer,
            &self.pixels,
            // Abgr8888 = RGBA byte order on little-endian, matching
            // tiny-skia's premultiplied RGBA pixel layout.
            smithay::backend::allocator::Fourcc::Abgr8888,
            (self.width as i32, HEADER_BAR_HEIGHT),
            false,
            1,
            smithay::utils::Transform::Normal,
            None,
        ) else {
            return vec![];
        };
        vec![TextureRenderElement::from_texture_buffer(
            location.to_f64(),
            &buffer,
            Some(alpha),
            None,
            Some((self.width as i32, HEADER_BAR_HEIGHT).into()),
            Kind::Unspecified,
        )
        .into()]
    }
}

impl WindowElement {
    pub fn decoration_state(&self) -> RefMut<'_, WindowState> {
        self.user_data().insert_if_missing(|| {
            RefCell::new(WindowState {
                is_ssd: false,
                header_bar: HeaderBar::default(),
            })
        });

        self.user_data()
            .get::<RefCell<WindowState>>()
            .unwrap()
            .borrow_mut()
    }

    pub fn set_ssd(&self, ssd: bool) {
        self.decoration_state().is_ssd = ssd;
    }
}

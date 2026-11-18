//! Solyra panel — the first shell surface.
//!
//! This is the P3 spike the plan asks for: one small `zwlr_layer_shell_v1`
//! surface that reserves screen space in `solyra-comp` (our niri fork) and
//! draws the Solyra top bar with the real palette. Workspace state comes from
//! niri's IPC socket, so the pills show what the compositor actually has
//! instead of a mock.
//!
//! Scope: one bar, left-to-right. No dock, launcher, or control centre yet.
//! The window is 32 logical pixels, anchored to the top edge, and sets an
//! exclusive zone so tiled windows start below it.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

const BAR_HEIGHT: u32 = 32;

// Tokens shared with base/solyra-config.kdl and docs/design-language.md.
const BG: (u8, u8, u8) = (0x12, 0x10, 0x16);
const HAIRLINE: (u8, u8, u8) = (0x2a, 0x24, 0x30);
const TEXT: (u8, u8, u8) = (0xdc, 0xd6, 0xe2);
const TEXT_DIM: (u8, u8, u8) = (0x8b, 0x83, 0x94);
const ACCENT: (u8, u8, u8) = (0xb4, 0x8a, 0xad);
const PILL: (u8, u8, u8) = (0x24, 0x1f, 0x2a);
const ON_ACCENT: (u8, u8, u8) = (0x14, 0x10, 0x18);

fn main() {
    if let Err(err) = run() {
        eprintln!("solyra-panel: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new((BAR_HEIGHT * 4096 * 4) as usize, &shm)?;

    let surface = compositor.create_surface(&qh);
    let layer_surface = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Top,
        Some("solyra-panel"),
        None,
    );
    layer_surface.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    layer_surface.set_exclusive_zone(BAR_HEIGHT as i32);
    layer_surface.set_size(0, BAR_HEIGHT);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer_surface.commit();

    let font = load_font();
    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        shm,
        layer_shell,
        pool,
        layer_surface,
        font,
        width: 1280,
        tick: 0,
        needs_redraw: false,
    };

    // The bar's clock has to keep moving, but the Wayland queue only wakes on
    // events. A periodic display sync is the cheapest wake-up that does not
    // need a second event loop.
    {
        let conn = conn.clone();
        let qh = qh.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(20));
            conn.display().sync(&qh, ());
            if conn.flush().is_err() {
                break;
            }
        });
    }

    loop {
        event_queue.blocking_dispatch(&mut app)?;
        if app.needs_redraw {
            app.needs_redraw = false;
            app.redraw(&conn);
        }
    }
}

fn load_font() -> Option<FontRef<'static>> {
    // Adwaita Sans ships in the image and reads like the Inter the design
    // calls for; the rest are fallbacks for a bare container.
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
        "/usr/share/fonts/open-sans/OpenSans-Regular.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = FontRef::try_from_slice(Box::leak(bytes.into_boxed_slice())) {
                return Some(font);
            }
        }
    }
    None
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    // Held for the lifetime of the surfaces they created; dropping either
    // would take the panel's protocol objects with it.
    #[allow(dead_code)]
    compositor: CompositorState,
    shm: Shm,
    #[allow(dead_code)]
    layer_shell: LayerShell,
    pool: SlotPool,
    layer_surface: LayerSurface,
    font: Option<FontRef<'static>>,
    width: u32,
    tick: u64,
    needs_redraw: bool,
}

impl App {
    fn redraw(&mut self, conn: &Connection) {
        let width = self.width.max(1);
        let height = BAR_HEIGHT;
        let stride = width as usize * 4;

        let Ok((buffer, canvas)) = self.pool.create_buffer(
            width as i32,
            height as i32,
            stride as i32,
            Format::Argb8888,
        ) else {
            return;
        };
        let pixels = canvas;
        pixels.fill(0);

        fill(pixels, stride, 0, 0, width, height, BG);
        fill(pixels, stride, 0, height as i32 - 1, width, 1, HAIRLINE);

        // Brand mark: accent dot, then the product name.
        circle(pixels, stride, 16, 16, 4, ACCENT);
        if let Some(font) = &self.font {
            text(pixels, stride, width, height, font, "Solyra", 28, 21, 13.0, TEXT);
        }

        // Workspace pills, straight from niri's IPC.
        let workspaces = niri_workspaces();
        let mut x = 96;
        for (idx, active) in workspaces {
            let (fg, bg) = if active { (ON_ACCENT, ACCENT) } else { (TEXT_DIM, PILL) };
            round_rect(pixels, stride, x, 9, 18, 14, 7, bg);
            if let Some(font) = &self.font {
                let label = idx.to_string();
                let w = text_width(font, &label, 11.0);
                text(
                    pixels,
                    stride,
                    width,
                    height,
                    font,
                    &label,
                    x + (18 - w) / 2,
                    20,
                    11.0,
                    fg,
                );
            }
            x += 24;
        }

        // Clock on the right edge.
        if let Some(font) = &self.font {
            let now = chrono::Local::now();
            let clock = now.format("%H:%M").to_string();
            let day = now.format("%a %d %b").to_string();
            let clock_w = text_width(font, &clock, 13.0);
            let day_w = text_width(font, &day, 11.5);
            let right = width as i32 - 16;
            text(
                pixels,
                stride,
                width,
                height,
                font,
                &clock,
                right - clock_w,
                21,
                13.0,
                TEXT,
            );
            text(
                pixels,
                stride,
                width,
                height,
                font,
                &day,
                right - clock_w - day_w - 12,
                20,
                11.5,
                TEXT_DIM,
            );
        }

        let _ = &self.tick;
        self.tick = self.tick.wrapping_add(1);

        let surface = self.layer_surface.wl_surface();
        if buffer.attach_to(surface).is_ok() {
            surface.damage_buffer(0, 0, width as i32, height as i32);
            surface.commit();
        }
        let _ = conn.flush();
    }
}

/// Ask niri for its workspaces. Returns `(index, is_active)` for indices 1..=6.
fn niri_workspaces() -> Vec<(u8, bool)> {
    let Some(path) = niri_socket() else {
        return (1..=3).map(|i| (i, i == 1)).collect();
    };
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => return (1..=3).map(|i| (i, i == 1)).collect(),
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    if stream.write_all(b"\"Workspaces\"\n").is_err() {
        return Vec::new();
    }
    let mut reply = String::new();
    if stream.read_to_string(&mut reply).is_err() && reply.is_empty() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(list) = value.as_array() {
        for ws in list {
            let idx = ws.get("idx").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let active = ws.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
            if idx >= 1 && idx <= 6 {
                out.push((idx, active));
            }
        }
    }
    out.sort();
    out.dedup_by_key(|(idx, _)| *idx);
    if out.is_empty() {
        (1..=3).map(|i| (i, i == 1)).collect()
    } else {
        out
    }
}

fn niri_socket() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NIRI_SOCKET") {
        return Some(PathBuf::from(path));
    }
    let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("niri.wayland-") && name.ends_with(".sock") {
            if let Ok(meta) = entry.metadata() {
                let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                    newest = Some((modified, entry.path()));
                }
            }
        }
    }
    newest.map(|(_, path)| path)
}

#[allow(clippy::too_many_arguments)]
fn text(
    pixels: &mut [u8],
    stride: usize,
    width: u32,
    height: u32,
    font: &FontRef<'_>,
    value: &str,
    x: i32,
    baseline: i32,
    size: f32,
    color: (u8, u8, u8),
) {
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let mut caret = point(x as f32, baseline as f32);
    for ch in value.chars() {
        let gid = scaled.glyph_id(ch);
        if let Some(outline) = scaled.outline_glyph(gid.with_scale_and_position(scale, caret)) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
                    blend(pixels, stride, px as usize, py as usize, color, coverage);
                }
            });
        }
        caret.x += scaled.h_advance(gid);
    }
}

fn text_width(font: &FontRef<'_>, value: &str, size: f32) -> i32 {
    let scaled = font.as_scaled(PxScale::from(size));
    value.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum::<f32>() as i32
}

fn blend(pixels: &mut [u8], stride: usize, x: usize, y: usize, color: (u8, u8, u8), coverage: f32) {
    let offset = y * stride + x * 4;
    if offset + 3 >= pixels.len() {
        return;
    }
    let alpha = coverage.clamp(0.0, 1.0);
    for (index, channel) in [color.0, color.1, color.2].into_iter().enumerate() {
        let dst = pixels[offset + index] as f32;
        pixels[offset + index] = (dst + (channel as f32 - dst) * alpha).round() as u8;
    }
    pixels[offset + 3] = 255;
}

fn fill(
    pixels: &mut [u8],
    stride: usize,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: (u8, u8, u8),
) {
    for row in y..y + h as i32 {
        for col in x..x + w as i32 {
            if row < 0 || col < 0 {
                continue;
            }
            blend(pixels, stride, col as usize, row as usize, color, 1.0);
        }
    }
}

fn circle(pixels: &mut [u8], stride: usize, cx: i32, cy: i32, radius: i32, color: (u8, u8, u8)) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let distance = ((dx * dx + dy * dy) as f32).sqrt();
            let coverage = (radius as f32 + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 && cx + dx >= 0 && cy + dy >= 0 {
                blend(
                    pixels,
                    stride,
                    (cx + dx) as usize,
                    (cy + dy) as usize,
                    color,
                    coverage,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn round_rect(
    pixels: &mut [u8],
    stride: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    color: (u8, u8, u8),
) {
    for row in 0..h {
        for col in 0..w {
            let px = x + col;
            let py = y + row;
            if px < 0 || py < 0 {
                continue;
            }
            // Distance to the nearest rounded corner, for a soft edge.
            let dx = (radius - col).max(col - (w - radius - 1)).max(0);
            let dy = (radius - row).max(row - (h - radius - 1)).max(0);
            let distance = ((dx * dx + dy * dy) as f32).sqrt();
            let coverage = if distance <= radius as f32 {
                1.0
            } else {
                (radius as f32 + 0.5 - distance).clamp(0.0, 1.0)
            };
            if coverage > 0.0 {
                blend(pixels, stride, px as usize, py as usize, color, coverage);
            }
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        std::process::exit(0);
    }

    fn configure(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 > 0 {
            self.width = configure.new_size.0;
        }
        let _ = conn;
        self.needs_redraw = true;
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

impl Dispatch<WlCallback, ()> for App {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: <WlCallback as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_callback::Event::Done { .. } = event {
            // The periodic display sync is only a wake-up: the clock has
            // moved, so repaint on the way out of dispatch.
            state.needs_redraw = true;
        }
    }
}

smithay_client_toolkit::delegate_compositor!(App);
smithay_client_toolkit::delegate_shm!(App);
smithay_client_toolkit::delegate_output!(App);
smithay_client_toolkit::delegate_layer!(App);
smithay_client_toolkit::delegate_registry!(App);

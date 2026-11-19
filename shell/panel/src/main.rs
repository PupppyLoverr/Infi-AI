//! Solyra shell — bar, island, and dock.
//!
//! Three surfaces, one process:
//!
//! * **bar** — 32 px, exclusive zone, menus left, status right, a reserved
//!   notch in the centre, and the dynamic island that grows out of it.
//! * **dock** — the hybrid: a floating card with a start cell, one cell per
//!   running app, and an agent shelf that only appears when agents exist.
//! * **island** — not a surface of its own yet: it is drawn inside the bar,
//!   expanding horizontally out of the notch. `solyra-panel notify "text"`
//!   puts a message in it for eight seconds.
//!
//! Pointer input is real: clicking a workspace pill switches workspace, a dock
//! cell focuses that app (or launches it), the start cell opens the launcher.
//! Everything goes through niri's IPC socket, so the shell owns no state the
//! compositor does not.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

const BAR_HEIGHT: u32 = 32;
const NOTCH_WIDTH: u32 = 186;
const NOTCH_HEIGHT: u32 = 26;
const NOTCH_RADIUS: i32 = 14;
const DOCK_HEIGHT: u32 = 56;
const CELL: i32 = 34;
const GAP: i32 = 6;
const ISLAND_TTL: u64 = 8;

// Tokens shared with base/solyra-config.kdl and docs/design-language.md.
const BG: (u8, u8, u8) = (0x12, 0x10, 0x16);
const HAIRLINE: (u8, u8, u8) = (0x2a, 0x24, 0x30);
const TEXT: (u8, u8, u8) = (0xdc, 0xd6, 0xe2);
const TEXT_DIM: (u8, u8, u8) = (0x8b, 0x83, 0x94);
const ACCENT: (u8, u8, u8) = (0xb4, 0x8a, 0xad);
const PILL: (u8, u8, u8) = (0x24, 0x1f, 0x2a);
const ON_ACCENT: (u8, u8, u8) = (0x14, 0x10, 0x18);
const VOID: (u8, u8, u8) = (0x07, 0x06, 0x0a);
const LENS: (u8, u8, u8) = (0x1b, 0x27, 0x33);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("notify") {
        let text = args[2..].join(" ");
        if let Err(err) = notify(&text) {
            eprintln!("solyra-panel notify: {err}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(err) = run() {
        eprintln!("solyra-panel: {err}");
        std::process::exit(1);
    }
}

fn runtime_path(name: &str) -> Option<PathBuf> {
    let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(PathBuf::from(dir).join(name))
}

fn notify(text: &str) -> std::io::Result<()> {
    let path = runtime_path("solyra-island").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set")
    })?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::fs::write(path, format!("{now}\t{text}\n"))
}

fn read_island() -> Option<String> {
    let path = runtime_path("solyra-island")?;
    let raw = std::fs::read_to_string(path).ok()?;
    let (stamp, text) = raw.split_once('\t')?;
    let stamp: u64 = stamp.trim().parse().ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(stamp) > ISLAND_TTL || text.trim().is_empty() {
        return None;
    }
    Some(text.trim().to_string())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new((BAR_HEIGHT.max(DOCK_HEIGHT) * 4096 * 4) as usize, &shm)?;
    let seat = SeatState::new(&globals, &qh);

    let bar = make_surface(
        &compositor,
        &layer_shell,
        &qh,
        Layer::Top,
        "solyra-bar",
        Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        BAR_HEIGHT as i32,
        0,
        BAR_HEIGHT,
    );
    let dock = make_surface(
        &compositor,
        &layer_shell,
        &qh,
        Layer::Top,
        "solyra-dock",
        Anchor::BOTTOM,
        0,
        1,
        DOCK_HEIGHT,
    );

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: seat,
        compositor,
        shm,
        layer_shell,
        pool,
        bar,
        dock,
        pointer: None,
        pointer_surface: None,
        pointer_pos: (0.0, 0.0),
        running: Vec::new(),
        pinned: pinned_apps(),
        island: None,
        icons: HashMap::new(),
        font: load_font(),
        needs_redraw: true,
    };

    // One wake-up per second: keeps the clock honest, notices a new island
    // message, and picks up windows opening or closing.
    {
        let conn = conn.clone();
        let qh = qh.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
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
            app.refresh_state();
            app.redraw(&conn);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn make_surface(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<App>,
    layer: Layer,
    namespace: &str,
    anchor: Anchor,
    exclusive: i32,
    width: u32,
    height: u32,
) -> SurfaceState {
    let wl_surface = compositor.create_surface(qh);
    let layer_surface = layer_shell.create_layer_surface(
        qh,
        wl_surface.clone(),
        layer,
        Some(namespace),
        None,
    );
    layer_surface.set_anchor(anchor);
    layer_surface.set_exclusive_zone(exclusive);
    layer_surface.set_size(width, height);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer_surface.commit();
    SurfaceState {
        surface: wl_surface,
        layer_surface,
        width,
        height,
        regions: Vec::new(),
    }
}

struct SurfaceState {
    surface: WlSurface,
    layer_surface: LayerSurface,
    width: u32,
    #[allow(dead_code)]
    height: u32,
    regions: Vec<Region>,
}

#[derive(Clone)]
struct Region {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    action: Action,
}

#[derive(Clone)]
enum Action {
    Workspace(u8),
    FocusWindow(u64),
    Spawn(String),
}

#[derive(Clone)]
struct AppEntry {
    window: Option<u64>,
    app_id: String,
    title: String,
    command: String,
}

impl AppEntry {
    fn glyph(&self) -> String {
        let first = self
            .title
            .chars()
            .next()
            .or_else(|| self.app_id.chars().next())
            .unwrap_or('?');
        first.to_uppercase().to_string()
    }
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    // Kept alive: dropping either takes the protocol objects with it.
    #[allow(dead_code)]
    compositor: CompositorState,
    shm: Shm,
    #[allow(dead_code)]
    layer_shell: LayerShell,
    pool: SlotPool,
    bar: SurfaceState,
    dock: SurfaceState,
    pointer: Option<WlPointer>,
    pointer_surface: Option<WlSurface>,
    pointer_pos: (f64, f64),
    running: Vec<AppEntry>,
    pinned: Vec<AppEntry>,
    island: Option<String>,
    icons: HashMap<String, Option<Icon>>,
    font: Option<FontRef<'static>>,
    needs_redraw: bool,
}

impl App {
    fn refresh_state(&mut self) {
        self.running = running_apps();
        self.island = read_island();
        // The dock sizes to its content; a resize is a configure round-trip.
        let wanted = dock_width(&self.running, &self.pinned);
        if wanted != self.dock.width {
            self.dock.width = wanted;
            self.dock
                .layer_surface
                .set_size(wanted, DOCK_HEIGHT);
            self.dock.layer_surface.commit();
        }
    }

    fn redraw(&mut self, conn: &Connection) {
        self.draw_bar(conn);
        self.draw_dock(conn);
        let _ = conn.flush();
    }

    fn draw_bar(&mut self, conn: &Connection) {
        let width = self.bar.width.max(1);
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
        canvas.fill(0);
        let mut regions = Vec::new();

        vgrad(canvas, stride, 0, 0, width, height, (0x18, 0x14, 0x1d), BG);
        fill(canvas, stride, 0, height as i32 - 1, width, 1, HAIRLINE);

        // Reserved notch: a void with a fake lens, so hardware with and
        // without a camera has the same geometry.
        notch(canvas, stride, width, NOTCH_WIDTH, NOTCH_HEIGHT, NOTCH_RADIUS, VOID);
        circle(canvas, stride, width as i32 / 2, 11, 3, LENS);

        // Brand.
        circle(canvas, stride, 16, 16, 4, ACCENT);
        if let Some(font) = self.font.as_ref() {
            text(canvas, stride, width, height, font, "Solyra", 28, 21, 13.0, TEXT);
        }

        // Workspace pills, clickable.
        let mut x = 96;
        for (idx, active) in workspaces() {
            let (fg, bg) = if active { (ON_ACCENT, ACCENT) } else { (TEXT_DIM, PILL) };
            round_rect(canvas, stride, x, 9, 18, 14, 7, bg);
            if let Some(font) = self.font.as_ref() {
                let label = idx.to_string();
                let w = text_width(font, &label, 11.0);
                text(canvas, stride, width, height, font, &label, x + (18 - w) / 2, 20, 11.0, fg);
            }
            regions.push(Region { x, y: 6, w: 18, h: 20, action: Action::Workspace(idx) });
            x += 24;
        }

        // Island: grows out of the notch when something needs attention.
        if let Some(message) = &self.island {
            if let Some(font) = self.font.as_ref() {
                let text_w = text_width(font, message, 11.5);
                let pill_w = (text_w + 46).min(420);
                let left = (width as i32 - pill_w) / 2;
                round_rect(canvas, stride, left, 4, pill_w, 24, 12, (0x0a, 0x09, 0x10));
                circle(canvas, stride, left + 14, 16, 3, ACCENT);
                let label = if text_w > pill_w - 40 {
                    let mut s = message.clone();
                    while text_width(font, &s, 11.5) > pill_w - 40 && s.len() > 4 {
                        s.pop();
                    }
                    s
                } else {
                    message.clone()
                };
                text(canvas, stride, width, height, font, &label, left + 26, 20, 11.5, TEXT);
            }
        }

        // Clock and date.
        if let Some(font) = self.font.as_ref() {
            let now = chrono::Local::now();
            let clock = now.format("%H:%M").to_string();
            let day = now.format("%a %d %b").to_string();
            let clock_w = text_width(font, &clock, 13.0);
            let day_w = text_width(font, &day, 11.5);
            let right = width as i32 - 16;
            text(canvas, stride, width, height, font, &clock, right - clock_w, 21, 13.0, TEXT);
            text(canvas, stride, width, height, font, &day, right - clock_w - day_w - 12, 20, 11.5, TEXT_DIM);
        }

        self.bar.regions = regions;
        let _ = conn;
        let surface = self.bar.surface.clone();
        if buffer.attach_to(&surface).is_ok() {
            surface.damage_buffer(0, 0, width as i32, height as i32);
            surface.commit();
        }
    }

    fn draw_dock(&mut self, conn: &Connection) {
        let width = self.dock.width.max(1);
        let height = DOCK_HEIGHT;
        let stride = width as usize * 4;
        let Ok((buffer, canvas)) = self.pool.create_buffer(
            width as i32,
            height as i32,
            stride as i32,
            Format::Argb8888,
        ) else {
            return;
        };
        canvas.fill(0);
        let mut regions = Vec::new();

        // Floating card: 8 px inset, 16 px radius, soft vertical gradient.
        vgrad_round(canvas, stride, 0, 8, width as i32, height as i32 - 8, 16,
                    (0x26, 0x21, 0x2e), (0x19, 0x16, 0x1f));
        fill(canvas, stride, 12, 9, width.saturating_sub(24), 1, (0x3a, 0x33, 0x44));

        let mut x = 10;
        // Start cell.
        round_rect(canvas, stride, x, 15, CELL, CELL, 10, ACCENT);
        if let Some(font) = self.font.as_ref() {
            let w = text_width(font, "◈", 14.0);
            text(canvas, stride, width, height, font, "◈", x + (CELL - w) / 2, 38, 14.0, ON_ACCENT);
        }
        regions.push(Region { x, y: 8, w: CELL, h: CELL, action: Action::Spawn("fuzzel".into()) });
        x += CELL + GAP;

        // Running apps first, then pinned ones that are not running.
        let mut cells: Vec<(AppEntry, bool)> = Vec::new();
        for app in &self.running {
            cells.push((app.clone(), true));
        }
        for pin in &self.pinned {
            if !self.running.iter().any(|a| a.app_id == pin.app_id) {
                cells.push((pin.clone(), false));
            }
        }
        for (index, (app, running)) in cells.iter().enumerate() {
            if x + CELL > width as i32 - 10 {
                break;
            }
            let hovered = self.pointer_surface.as_ref() == Some(&self.dock.surface)
                && self.pointer_pos.0 >= x as f64
                && self.pointer_pos.0 < (x + CELL) as f64
                && self.pointer_pos.1 >= 15.0
                && self.pointer_pos.1 < (15 + CELL) as f64;
            let base = if hovered { (0x3a, 0x32, 0x45) } else { (0x2a, 0x25, 0x33) };
            round_rect(canvas, stride, x, 15, CELL, CELL, 10, base);
            if hovered {
                round_rect(canvas, stride, x, 15, CELL, 2, 1, ACCENT);
            }
            let _ = index;
            if !self.icons.contains_key(&app.app_id) {
                let loaded = icon_for(&app.app_id);
                self.icons.insert(app.app_id.clone(), loaded);
            }
            let icon = self.icons.get(&app.app_id).and_then(|i| i.as_ref());
            match icon {
                Some(icon) => draw_icon(canvas, stride, x + 5, 20, CELL - 10, icon),
                None => {
                    if let Some(font) = self.font.as_ref() {
                        let glyph = app.glyph();
                        let w = text_width(font, &glyph, 13.0);
                        text(canvas, stride, width, height, font, &glyph, x + (CELL - w) / 2, 38, 13.0, (0xf2, 0xee, 0xf6));
                    }
                }
            }
            if *running {
                round_rect(canvas, stride, x + 11, height as i32 - 6, CELL - 22, 2, 1, ACCENT);
            }
            let action = match app.window {
                Some(id) => Action::FocusWindow(id),
                None => Action::Spawn(app.command.clone()),
            };
            regions.push(Region { x, y: 8, w: CELL, h: CELL, action });
            x += CELL + GAP;
        }

        self.dock.regions = regions;
        let _ = conn;
        let surface = self.dock.surface.clone();
        if buffer.attach_to(&surface).is_ok() {
            surface.damage_buffer(0, 0, width as i32, height as i32);
            surface.commit();
        }
    }

    fn click(&mut self, surface: &WlSurface, x: f64, y: f64) {
        let target = if surface == &self.bar.surface {
            &self.bar
        } else if surface == &self.dock.surface {
            &self.dock
        } else {
            return;
        };
        let hit = target
            .regions
            .iter()
            .find(|r| {
                x >= r.x as f64
                    && x < (r.x + r.w) as f64
                    && y >= r.y as f64
                    && y < (r.y + r.h) as f64
            })
            .map(|r| r.action.clone());
        match hit {
            Some(Action::Workspace(idx)) => {
                send_ipc(&format!(
                    "{{\"Action\":{{\"FocusWorkspace\":{{\"reference\":{{\"Index\":{idx}}}}}}}}}"
                ));
            }
            Some(Action::FocusWindow(id)) => {
                send_ipc(&format!("{{\"Action\":{{\"FocusWindow\":{{\"id\":{id}}}}}}}"));
            }
            Some(Action::Spawn(command)) => {
                let _ = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("{command} >/dev/null 2>&1 &"))
                    .spawn();
            }
            None => {}
        }
        self.needs_redraw = true;
    }
}

fn dock_width(running: &[AppEntry], pinned: &[AppEntry]) -> u32 {
    let running_ids: Vec<&str> = running.iter().map(|a| a.app_id.as_str()).collect();
    let extra = pinned
        .iter()
        .filter(|p| !running_ids.contains(&p.app_id.as_str()))
        .count();
    let cells = 1 + running.len() + extra;
    (20 + cells as i32 * CELL + (cells as i32 - 1) * GAP).max(120) as u32
}


// ------------------------------------------------------------------- icons

/// An RGBA icon decoded from the system icon theme.
struct Icon {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

const ICON_DIRS: &[&str] = &[
    "/usr/share/icons/hicolor/48x48/apps",
    "/usr/share/icons/hicolor/64x64/apps",
    "/usr/share/icons/hicolor/128x128/apps",
    "/usr/share/icons/Adwaita/48x48/apps",
    "/usr/share/icons/Adwaita/64x64/apps",
];

/// Find an icon for an app id. Desktop files are the authority when they
/// exist; otherwise a few naming conventions cover the common cases.
fn icon_for(app_id: &str) -> Option<Icon> {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entry) = std::fs::read_to_string(format!("/usr/share/applications/{app_id}.desktop")) {
        for line in entry.lines() {
            if let Some(icon) = line.strip_prefix("Icon=") {
                names.push(icon.trim().to_string());
                break;
            }
        }
    }
    names.push(app_id.to_string());
    names.push(format!("org.xfce.{app_id}"));
    let mut capitalised = app_id.to_string();
    if let Some(first) = capitalised.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    names.push(capitalised);

    for name in names {
        for dir in ICON_DIRS {
            let path = format!("{dir}/{name}.png");
            if let Ok(bytes) = std::fs::read(&path) {
                if let Some(icon) = decode_png(&bytes) {
                    return Some(icon);
                }
            }
        }
    }
    None
}

fn decode_png(bytes: &[u8]) -> Option<Icon> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
    match info.color_type {
        png::ColorType::Rgba => rgba = buf,
        png::ColorType::Rgb => {
            for px in buf.chunks(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        _ => return None,
    }
    Some(Icon { width: info.width, height: info.height, pixels: rgba })
}

/// Draw an icon into a square box with bilinear sampling and alpha blending.
fn draw_icon(pixels: &mut [u8], stride: usize, x: i32, y: i32, box_px: i32, icon: &Icon) {
    if icon.width == 0 || icon.height == 0 {
        return;
    }
    let scale_x = icon.width as f32 / box_px as f32;
    let scale_y = icon.height as f32 / box_px as f32;
    for dy in 0..box_px {
        for dx in 0..box_px {
            let sx = (dx as f32 + 0.5) * scale_x - 0.5;
            let sy = (dy as f32 + 0.5) * scale_y - 0.5;
            let (x0, y0) = (sx.floor().max(0.0) as u32, sy.floor().max(0.0) as u32);
            let (x1, y1) = ((x0 + 1).min(icon.width - 1), (y0 + 1).min(icon.height - 1));
            let fx = (sx - x0 as f32).clamp(0.0, 1.0);
            let fy = (sy - y0 as f32).clamp(0.0, 1.0);
            let mut sample = [0f32; 4];
            for (ix, iy, weight) in [
                (x0, y0, (1.0 - fx) * (1.0 - fy)),
                (x1, y0, fx * (1.0 - fy)),
                (x0, y1, (1.0 - fx) * fy),
                (x1, y1, fx * fy),
            ] {
                let offset = ((iy * icon.width + ix) * 4) as usize;
                for channel in 0..4 {
                    sample[channel] += icon.pixels[offset + channel] as f32 * weight;
                }
            }
            let alpha = sample[3] / 255.0;
            if alpha <= 0.004 {
                continue;
            }
            let color = (sample[0] as u8, sample[1] as u8, sample[2] as u8);
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && py >= 0 {
                blend(pixels, stride, px as usize, py as usize, color, alpha);
            }
        }
    }
}

// ---------------------------------------------------------------- niri IPC

fn niri_socket() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NIRI_SOCKET") {
        return Some(PathBuf::from(path));
    }
    let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("niri.wayland-") && name.ends_with(".sock") {
            if let Ok(meta) = entry.metadata() {
                let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                    newest = Some((modified, entry.path()));
                }
            }
        }
    }
    newest.map(|(_, path)| path)
}

fn ipc(request: &str) -> Option<String> {
    let path = niri_socket()?;
    let mut stream = UnixStream::connect(path).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(400))).ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    Some(reply)
}

fn send_ipc(request: &str) {
    let _ = ipc(request);
}

fn workspaces() -> Vec<(u8, bool)> {
    let Some(reply) = ipc("\"Workspaces\"") else {
        return (1..=3).map(|i| (i, i == 1)).collect();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply) else {
        return (1..=3).map(|i| (i, i == 1)).collect();
    };
    let mut out = Vec::new();
    if let Some(list) = value.as_array() {
        for ws in list {
            let idx = ws.get("idx").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let active = ws.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
            if (1..=6).contains(&idx) {
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

fn running_apps() -> Vec<AppEntry> {
    let Some(reply) = ipc("\"Windows\"") else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply) else {
        return Vec::new();
    };
    let mut out: Vec<AppEntry> = Vec::new();
    if let Some(list) = value.as_array() {
        for win in list {
            let app_id = win.get("app_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if app_id.is_empty() {
                continue;
            }
            let title = win.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let id = win.get("id").and_then(|v| v.as_u64());
            if out.iter().any(|a| a.app_id == app_id) {
                continue;
            }
            out.push(AppEntry { window: id, app_id: app_id.clone(), title, command: app_id });
        }
    }
    out
}

// ------------------------------------------------------------- roles file

fn role_binary(role: &str, fallback: &str) -> String {
    let mut paths = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(home).join(".config/solyra/defaults.toml"));
    }
    paths.push(PathBuf::from("/etc/solyra/defaults.toml"));
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() != role {
                continue;
            }
            let value = value.trim().trim_matches('"');
            let binary = value.trim_end_matches(".desktop");
            if !binary.is_empty() {
                return binary.to_string();
            }
        }
    }
    fallback.to_string()
}

fn pinned_apps() -> Vec<AppEntry> {
    let terminal = role_binary("terminal", "foot");
    let files = role_binary("files", "thunar");
    vec![
        AppEntry { window: None, app_id: terminal.clone(), title: terminal.clone(), command: terminal },
        AppEntry { window: None, app_id: files.clone(), title: files.clone(), command: files },
    ]
}

// ----------------------------------------------------------------- drawing

fn load_font() -> Option<FontRef<'static>> {
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

#[allow(clippy::too_many_arguments)]
fn vgrad(
    pixels: &mut [u8],
    stride: usize,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    top: (u8, u8, u8),
    bottom: (u8, u8, u8),
) {
    for row in 0..h as i32 {
        let t = row as f32 / (h.max(2) as f32 - 1.0);
        let color = (
            (top.0 as f32 + (bottom.0 as f32 - top.0 as f32) * t).round() as u8,
            (top.1 as f32 + (bottom.1 as f32 - top.1 as f32) * t).round() as u8,
            (top.2 as f32 + (bottom.2 as f32 - top.2 as f32) * t).round() as u8,
        );
        fill(pixels, stride, x, y + row, w, 1, color);
    }
}

/// A rounded rectangle filled with a vertical gradient.
#[allow(clippy::too_many_arguments)]
fn vgrad_round(
    pixels: &mut [u8],
    stride: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    top: (u8, u8, u8),
    bottom: (u8, u8, u8),
) {
    for row in 0..h {
        let t = row as f32 / (h.max(2) as f32 - 1.0);
        let color = (
            (top.0 as f32 + (bottom.0 as f32 - top.0 as f32) * t).round() as u8,
            (top.1 as f32 + (bottom.1 as f32 - top.1 as f32) * t).round() as u8,
            (top.2 as f32 + (bottom.2 as f32 - top.2 as f32) * t).round() as u8,
        );
        let dy = (radius - row).max(row - (h - radius - 1)).max(0);
        for col in 0..w {
            let dx = (radius - col).max(col - (w - radius - 1)).max(0);
            let distance = ((dx * dx + dy * dy) as f32).sqrt();
            let coverage = if distance <= radius as f32 {
                1.0
            } else {
                (radius as f32 + 0.5 - distance).clamp(0.0, 1.0)
            };
            if coverage > 0.0 {
                blend(pixels, stride, (x + col) as usize, (y + row) as usize, color, coverage);
            }
        }
    }
}

fn fill(pixels: &mut [u8], stride: usize, x: i32, y: i32, w: u32, h: u32, color: (u8, u8, u8)) {
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
                blend(pixels, stride, (cx + dx) as usize, (cy + dy) as usize, color, coverage);
            }
        }
    }
}

fn notch(
    pixels: &mut [u8],
    stride: usize,
    screen_width: u32,
    width: u32,
    height: u32,
    radius: i32,
    color: (u8, u8, u8),
) {
    let left = (screen_width as i32 - width as i32) / 2;
    let r = radius as f32;
    let corner_start = height as f32 - r;
    for row in 0..height as i32 {
        for col in 0..width as i32 {
            let mut inside = true;
            let dy = row as f32 - corner_start;
            if dy > 0.0 {
                let dx = if (col as i32) < radius {
                    r - col as f32
                } else if (col as i32) >= (width as i32 - radius) {
                    col as f32 - (width as f32 - r - 1.0)
                } else {
                    0.0
                };
                inside = dx * dx + dy * dy <= r * r;
            }
            if inside {
                blend(pixels, stride, (left + col) as usize, row as usize, color, 1.0);
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

// --------------------------------------------------------------- protocols

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: i32) {}
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        // Losing one surface is not fatal; losing the bar is.
        if layer.wl_surface() == &self.bar.surface {
            std::process::exit(0);
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if layer.wl_surface() == &self.bar.surface {
            if configure.new_size.0 > 0 {
                self.bar.width = configure.new_size.0;
            }
        } else if configure.new_size.0 > 0 {
            self.dock.width = configure.new_size.0;
        }
        self.needs_redraw = true;
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(pointer) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(pointer);
            }
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match &event.kind {
                PointerEventKind::Enter { .. } => {
                    self.pointer_surface = Some(event.surface.clone());
                    self.pointer_pos = event.position;
                }
                PointerEventKind::Leave { .. } => {
                    self.pointer_surface = None;
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_pos = event.position;
                    self.needs_redraw = true;
                }
                PointerEventKind::Press { button, .. } if *button == 0x110 => {
                    let surface = event.surface.clone();
                    let (x, y) = event.position;
                    self.click(&surface, x, y);
                }
                _ => {}
            }
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
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
            state.needs_redraw = true;
        }
    }
}

smithay_client_toolkit::delegate_compositor!(App);
smithay_client_toolkit::delegate_shm!(App);
smithay_client_toolkit::delegate_output!(App);
smithay_client_toolkit::delegate_layer!(App);
smithay_client_toolkit::delegate_registry!(App);
smithay_client_toolkit::delegate_seat!(App);
smithay_client_toolkit::delegate_pointer!(App);

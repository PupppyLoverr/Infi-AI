//! Solyra shell (`solyra-panel`) — unified Wayland desktop shell:
//!
//! * **Top Bar** (`solyra-bar`): translucent glass menu bar with Solyra mark,
//!   focused app & contextual menus, workspace pills, Apple-style notch +
//!   Dynamic Island, Search button, Status capsule, and clock.
//! * **Dock** (`solyra-dock` / `solyra-taskbar`): all 3 variants live-switchable
//!   (`solyra` hybrid default, `macos` Sequoia dock, `windows` Fluent taskbar).
//! * **Launcher** (`solyra-launcher`): Spotlight + Start hybrid overlay with
//!   real keyboard search, Pinned Apps grid, and session footer.
//! * **Control Centre** (`solyra-control`): quick toggles, sliders, live Dock
//!   variant switcher, live Wallpaper switcher, and memory telemetry.
//! * **Expanded Island** (`solyra-island`): interactive Agent Space popover.

mod canvas;
mod state;
mod surfaces;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use canvas::{Canvas, Fonts, Icon};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use state::{
    apply_wallpaper, read_island_file, read_telemetry, send_client_cmd, send_ipc,
    spawn_ipc_listener, windows, workspaces, DockStyle, LaunchTarget, ShellCmd, ShellPrefs,
    WindowInfo,
};
use surfaces::{
    draw_bar, draw_control, draw_dock, draw_island_card, draw_launcher, filtered_launcher_apps,
    floating_dock_width, HitAction, Region, BAR_HEIGHT, CONTROL_H, CONTROL_W, DOCK_FLOAT_HEIGHT,
    ISLAND_CARD_H, ISLAND_CARD_W, LAUNCHER_H, LAUNCHER_W, TASKBAR_HEIGHT,
};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_keyboard::{self, KeyState, WlKeyboard};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(sub) = args.get(1).map(String::as_str) {
        let cmd_line = match sub {
            "notify" => Some(format!("notify {}", args[2..].join(" "))),
            "toggle-launcher" | "launcher" => Some("toggle-launcher".into()),
            "toggle-control" | "control" => Some("toggle-control".into()),
            "toggle-island" | "island" => Some("toggle-island".into()),
            "close" => Some("close".into()),
            "dock" => args.get(2).map(|v| format!("dock {v}")),
            "wallpaper" => args.get(2).map(|v| format!("wallpaper {v}")),
            _ => None,
        };
        if let Some(line) = cmd_line {
            if let Err(err) = send_client_cmd(&line) {
                eprintln!("solyra-panel {sub}: {err}");
                std::process::exit(1);
            }
            return;
        }
    }

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
    let pool = SlotPool::new(8 * 1024 * 1024, &shm)?;
    let seat_state = SeatState::new(&globals, &qh);

    let prefs = ShellPrefs::load();
    let bar = make_layer_surface(
        &compositor,
        &layer_shell,
        &qh,
        Layer::Top,
        "solyra-bar",
        Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        BAR_HEIGHT as i32,
        (0, 0, 0, 0),
        0,
        BAR_HEIGHT,
        KeyboardInteractivity::None,
    );
    let dock = make_dock_surface(&compositor, &layer_shell, &qh, prefs.dock_style, 320);

    let cmd_queue: Arc<Mutex<Vec<ShellCmd>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let conn_wake = conn.clone();
        let qh_wake = qh.clone();
        spawn_ipc_listener(cmd_queue.clone(), move || {
            conn_wake.display().sync(&qh_wake, ());
            let _ = conn_wake.flush();
        });
    }

    {
        let conn_tick = conn.clone();
        let qh_tick = qh.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            conn_tick.display().sync(&qh_tick, ());
            if conn_tick.flush().is_err() {
                break;
            }
        });
    }

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state,
        compositor,
        shm,
        layer_shell,
        qh: qh.clone(),
        pool,
        bar,
        dock,
        dock_active_style: prefs.dock_style,
        launcher: None,
        control: None,
        island_card: None,
        launcher_query: String::new(),
        launcher_selected: 0,
        shift_held: false,
        pointer: None,
        keyboard: None,
        pointer_surface: None,
        pointer_pos: (0.0, 0.0),
        prefs,
        cmd_queue,
        wins: Vec::new(),
        ws_list: vec![(1, true), (2, false), (3, false)],
        island_msg: None,
        island_override: None,
        icons: HashMap::new(),
        fonts: Fonts::load(),
        needs_redraw: true,
        needs_state_sync: true,
        last_sync: Instant::now() - Duration::from_secs(10),
    };

    loop {
        event_queue.blocking_dispatch(&mut app)?;
        app.drain_commands();
        if app.needs_redraw {
            app.needs_redraw = false;
            if app.needs_state_sync || app.last_sync.elapsed() >= Duration::from_millis(800) {
                app.needs_state_sync = false;
                app.last_sync = Instant::now();
                app.sync_compositor_state();
            }
            app.redraw(&conn);
        }
    }
}

struct SurfaceSlot {
    surface: WlSurface,
    layer: LayerSurface,
    width: u32,
    height: u32,
    configured: bool,
    regions: Vec<Region>,
}

#[allow(clippy::too_many_arguments)]
fn make_layer_surface(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<App>,
    layer: Layer,
    namespace: &str,
    anchor: Anchor,
    exclusive: i32,
    margin: (i32, i32, i32, i32),
    width: u32,
    height: u32,
    kb: KeyboardInteractivity,
) -> SurfaceSlot {
    let wl_surface = compositor.create_surface(qh);
    let ls = layer_shell.create_layer_surface(qh, wl_surface.clone(), layer, Some(namespace), None);
    ls.set_anchor(anchor);
    ls.set_exclusive_zone(exclusive);
    ls.set_margin(margin.0, margin.1, margin.2, margin.3);
    ls.set_size(width, height);
    ls.set_keyboard_interactivity(kb);
    ls.commit();
    SurfaceSlot {
        surface: wl_surface,
        layer: ls,
        width,
        height,
        configured: false,
        regions: Vec::new(),
    }
}

fn make_dock_surface(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<App>,
    style: DockStyle,
    initial_w: u32,
) -> SurfaceSlot {
    match style {
        DockStyle::Solyra | DockStyle::Macos => make_layer_surface(
            compositor,
            layer_shell,
            qh,
            Layer::Top,
            "solyra-dock",
            Anchor::BOTTOM,
            0,
            (0, 0, 10, 0),
            initial_w.max(220),
            DOCK_FLOAT_HEIGHT,
            KeyboardInteractivity::None,
        ),
        DockStyle::Windows => make_layer_surface(
            compositor,
            layer_shell,
            qh,
            Layer::Top,
            "solyra-taskbar",
            Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            TASKBAR_HEIGHT as i32,
            (0, 0, 0, 0),
            0,
            TASKBAR_HEIGHT,
            KeyboardInteractivity::None,
        ),
    }
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor: CompositorState,
    shm: Shm,
    layer_shell: LayerShell,
    qh: QueueHandle<App>,
    pool: SlotPool,

    bar: SurfaceSlot,
    dock: SurfaceSlot,
    dock_active_style: DockStyle,
    launcher: Option<SurfaceSlot>,
    control: Option<SurfaceSlot>,
    island_card: Option<SurfaceSlot>,

    launcher_query: String,
    launcher_selected: usize,
    shift_held: bool,

    pointer: Option<WlPointer>,
    keyboard: Option<WlKeyboard>,
    pointer_surface: Option<WlSurface>,
    pointer_pos: (f64, f64),

    prefs: ShellPrefs,
    cmd_queue: Arc<Mutex<Vec<ShellCmd>>>,
    wins: Vec<WindowInfo>,
    ws_list: Vec<(u8, bool)>,
    island_msg: Option<String>,
    island_override: Option<(Instant, String)>,
    icons: HashMap<String, Option<Icon>>,
    fonts: Fonts,
    needs_redraw: bool,
    needs_state_sync: bool,
    last_sync: Instant,
}

impl App {
    fn drain_commands(&mut self) {
        let cmds: Vec<ShellCmd> = if let Ok(mut q) = self.cmd_queue.lock() {
            std::mem::take(&mut *q)
        } else {
            Vec::new()
        };
        for cmd in cmds {
            self.apply_shell_cmd(cmd);
        }
    }

    fn apply_shell_cmd(&mut self, cmd: ShellCmd) {
        match cmd {
            ShellCmd::Notify(text) => {
                if text.trim().is_empty() {
                    self.island_override = None;
                } else {
                    self.island_override = Some((Instant::now(), text));
                }
                self.island_card = None;
            }
            ShellCmd::ToggleLauncher => {
                if self.launcher.is_some() {
                    self.launcher = None;
                } else {
                    self.control = None;
                    self.island_card = None;
                    self.launcher_query.clear();
                    self.launcher_selected = 0;
                    self.launcher = Some(make_layer_surface(
                        &self.compositor,
                        &self.layer_shell,
                        &self.qh,
                        Layer::Overlay,
                        "solyra-launcher",
                        Anchor::empty(),
                        0,
                        (0, 0, 0, 0),
                        LAUNCHER_W,
                        LAUNCHER_H,
                        KeyboardInteractivity::Exclusive,
                    ));
                }
            }
            ShellCmd::ToggleControl => {
                if self.control.is_some() {
                    self.control = None;
                } else {
                    self.launcher = None;
                    self.island_card = None;
                    self.control = Some(make_layer_surface(
                        &self.compositor,
                        &self.layer_shell,
                        &self.qh,
                        Layer::Overlay,
                        "solyra-control",
                        Anchor::TOP | Anchor::RIGHT,
                        0,
                        (8, 12, 0, 0),
                        CONTROL_W,
                        CONTROL_H,
                        KeyboardInteractivity::OnDemand,
                    ));
                }
            }
            ShellCmd::ToggleIsland => {
                if self.island_card.is_some() {
                    self.island_card = None;
                } else {
                    self.launcher = None;
                    self.control = None;
                    self.island_card = Some(make_layer_surface(
                        &self.compositor,
                        &self.layer_shell,
                        &self.qh,
                        Layer::Overlay,
                        "solyra-island",
                        Anchor::TOP,
                        0,
                        (6, 0, 0, 0),
                        ISLAND_CARD_W,
                        ISLAND_CARD_H,
                        KeyboardInteractivity::OnDemand,
                    ));
                }
            }
            ShellCmd::CloseOverlays => {
                self.launcher = None;
                self.control = None;
                self.island_card = None;
            }
            ShellCmd::SetDock(style) => {
                self.prefs.dock_style = style;
                self.prefs.save();
                if self.dock_active_style != style {
                    let wanted = floating_dock_width(style, &self.wins);
                    self.dock = make_dock_surface(
                        &self.compositor,
                        &self.layer_shell,
                        &self.qh,
                        style,
                        wanted,
                    );
                    self.dock_active_style = style;
                }
            }
            ShellCmd::SetWallpaper(preset) => {
                self.prefs.wallpaper = preset;
                self.prefs.save();
                apply_wallpaper(preset);
            }
        }
        self.needs_state_sync = true;
        self.needs_redraw = true;
    }

    fn sync_compositor_state(&mut self) {
        self.wins = windows();
        self.ws_list = workspaces();

        if let Some((t, ref msg)) = self.island_override {
            if t.elapsed() <= Duration::from_secs(state::ISLAND_TTL_SECS) {
                self.island_msg = Some(msg.clone());
            } else {
                self.island_override = None;
                self.island_msg = read_island_file();
            }
        } else {
            self.island_msg = read_island_file();
        }

        if matches!(self.dock_active_style, DockStyle::Solyra | DockStyle::Macos) {
            let wanted = floating_dock_width(self.dock_active_style, &self.wins);
            if wanted != self.dock.width {
                self.dock.width = wanted;
                self.dock.layer.set_size(wanted, DOCK_FLOAT_HEIGHT);
                self.dock.layer.commit();
            }
        }
    }

    fn redraw(&mut self, conn: &Connection) {
        let telem = read_telemetry();

        if self.bar.configured && self.bar.width > 0 {
            let hover = self.hover_on(&self.bar.surface);
            let w = self.bar.width;
            let h = self.bar.height;
            if let Ok((buffer, bytes)) =
                self.pool
                    .create_buffer(w as i32, h as i32, (w * 4) as i32, Format::Argb8888)
            {
                let mut cv = Canvas::new(bytes, w, h);
                self.bar.regions = draw_bar(
                    &mut cv,
                    &self.fonts,
                    &self.ws_list,
                    &self.wins,
                    self.island_msg.as_deref(),
                    &self.prefs,
                    &telem,
                    hover,
                );
                present(buffer, &self.bar.surface, w, h);
            }
        }

        if self.dock.configured && self.dock.width > 0 {
            let hover = self.hover_on(&self.dock.surface);
            let w = self.dock.width;
            let h = self.dock.height;
            if let Ok((buffer, bytes)) =
                self.pool
                    .create_buffer(w as i32, h as i32, (w * 4) as i32, Format::Argb8888)
            {
                let mut cv = Canvas::new(bytes, w, h);
                self.dock.regions = draw_dock(
                    &mut cv,
                    &self.fonts,
                    self.dock_active_style,
                    &self.wins,
                    &mut self.icons,
                    hover,
                );
                present(buffer, &self.dock.surface, w, h);
            }
        }

        if let Some(ref mut slot) = self.launcher {
            if slot.configured && slot.width > 0 {
                let hover = self
                    .pointer_surface
                    .as_ref()
                    .filter(|s| *s == &slot.surface)
                    .map(|_| self.pointer_pos);
                let w = slot.width;
                let h = slot.height;
                if let Ok((buffer, bytes)) =
                    self.pool
                        .create_buffer(w as i32, h as i32, (w * 4) as i32, Format::Argb8888)
                {
                    let mut cv = Canvas::new(bytes, w, h);
                    slot.regions = draw_launcher(
                        &mut cv,
                        &self.fonts,
                        &self.launcher_query,
                        self.launcher_selected,
                        &mut self.icons,
                        hover,
                    );
                    present(buffer, &slot.surface, w, h);
                }
            }
        }

        if let Some(ref mut slot) = self.control {
            if slot.configured && slot.width > 0 {
                let hover = self
                    .pointer_surface
                    .as_ref()
                    .filter(|s| *s == &slot.surface)
                    .map(|_| self.pointer_pos);
                let w = slot.width;
                let h = slot.height;
                if let Ok((buffer, bytes)) =
                    self.pool
                        .create_buffer(w as i32, h as i32, (w * 4) as i32, Format::Argb8888)
                {
                    let mut cv = Canvas::new(bytes, w, h);
                    slot.regions =
                        draw_control(&mut cv, &self.fonts, &self.prefs, &telem, hover);
                    present(buffer, &slot.surface, w, h);
                }
            }
        }

        if let Some(ref mut slot) = self.island_card {
            if slot.configured && slot.width > 0 {
                let w = slot.width;
                let h = slot.height;
                if let Ok((buffer, bytes)) =
                    self.pool
                        .create_buffer(w as i32, h as i32, (w * 4) as i32, Format::Argb8888)
                {
                    let mut cv = Canvas::new(bytes, w, h);
                    slot.regions = draw_island_card(
                        &mut cv,
                        &self.fonts,
                        self.island_msg.as_deref(),
                        &self.wins,
                        &telem,
                    );
                    present(buffer, &slot.surface, w, h);
                }
            }
        }

        let _ = conn.flush();
    }

    fn hover_on(&self, surface: &WlSurface) -> Option<(f64, f64)> {
        if self.pointer_surface.as_ref() == Some(surface) {
            Some(self.pointer_pos)
        } else {
            None
        }
    }

    fn handle_click(&mut self, surface: &WlSurface, x: f64, y: f64) {
        let regions = if surface == &self.bar.surface {
            &self.bar.regions
        } else if surface == &self.dock.surface {
            &self.dock.regions
        } else if self.launcher.as_ref().is_some_and(|s| &s.surface == surface) {
            &self.launcher.as_ref().unwrap().regions
        } else if self.control.as_ref().is_some_and(|s| &s.surface == surface) {
            &self.control.as_ref().unwrap().regions
        } else if self
            .island_card
            .as_ref()
            .is_some_and(|s| &s.surface == surface)
        {
            &self.island_card.as_ref().unwrap().regions
        } else {
            return;
        };

        let Some(hit) = regions.iter().find(|r| r.contains(x, y)).map(|r| r.action.clone()) else {
            return;
        };

        match hit {
            HitAction::Workspace(idx) => {
                send_ipc(&format!(
                    "{{\"Action\":{{\"FocusWorkspace\":{{\"reference\":{{\"Index\":{idx}}}}}}}}}"
                ));
                self.launcher = None;
            }
            HitAction::FocusWindow(id) => {
                send_ipc(&format!("{{\"Action\":{{\"FocusWindow\":{{\"id\":{id}}}}}}}"));
                self.launcher = None;
            }
            HitAction::Launch(target) => {
                self.execute_launch(target);
                self.launcher = None;
            }
            HitAction::Shell(cmd) => {
                self.apply_shell_cmd(cmd);
            }
            HitAction::ToggleWifi => {
                self.prefs.wifi_on = !self.prefs.wifi_on;
            }
            HitAction::ToggleBt => {
                self.prefs.bt_on = !self.prefs.bt_on;
            }
            HitAction::ToggleDnd => {
                self.prefs.dnd_on = !self.prefs.dnd_on;
            }
            HitAction::SetVolumeFromX { x0, w } => {
                let pct = (((x - x0 as f64) / w.max(1) as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8;
                self.prefs.volume = pct;
                self.prefs.save();
                let _ = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("wpctl set-volume @DEFAULT_AUDIO_SINK@ {}% >/dev/null 2>&1 &", pct))
                    .spawn();
            }
            HitAction::SetBrightnessFromX { x0, w } => {
                let pct = (((x - x0 as f64) / w.max(1) as f64) * 100.0)
                    .round()
                    .clamp(15.0, 100.0) as u8;
                self.prefs.brightness = pct;
                self.prefs.save();
            }
        }
        self.needs_state_sync = true;
        self.needs_redraw = true;
    }

    fn execute_launch(&mut self, target: LaunchTarget) {
        match target {
            LaunchTarget::Shell(cmd) => {
                let _ = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("{cmd} >/dev/null 2>&1 &"))
                    .spawn();
            }
            LaunchTarget::Ipc(msg) => {
                send_ipc(msg);
            }
            LaunchTarget::Internal(cmd) => {
                self.apply_shell_cmd(cmd);
            }
        }
    }

    fn handle_evdev_key(&mut self, key: u32, pressed: bool) {
        // Track Shift state (left shift = 42, right shift = 54).
        if key == 42 || key == 54 {
            self.shift_held = pressed;
            return;
        }
        if !pressed {
            return;
        }
        // Escape closes any open overlay.
        if key == 1 {
            self.apply_shell_cmd(ShellCmd::CloseOverlays);
            return;
        }
        if self.launcher.is_none() {
            return;
        }

        let items = filtered_launcher_apps(&self.launcher_query);
        let max_idx = items.len().saturating_sub(1);

        match key {
            14 => {
                // Backspace
                self.launcher_query.pop();
                self.launcher_selected = 0;
            }
            28 | 96 => {
                // Enter / KP_Enter
                if let Some(app) = items.get(self.launcher_selected.min(max_idx)).cloned() {
                    self.launcher = None;
                    self.execute_launch(app.command);
                } else if !self.launcher_query.trim().is_empty() {
                    let q = self.launcher_query.clone();
                    self.launcher = None;
                    self.execute_launch(LaunchTarget::Shell(q));
                }
            }
            103 | 105 => {
                // Up / Left
                self.launcher_selected = self.launcher_selected.saturating_sub(1);
            }
            108 | 106 | 15 => {
                // Down / Right / Tab
                self.launcher_selected = (self.launcher_selected + 1).min(max_idx);
            }
            _ => {
                if let Some(ch) = evdev_to_ascii(key, self.shift_held) {
                    self.launcher_query.push(ch);
                    self.launcher_selected = 0;
                }
            }
        }
        self.needs_redraw = true;
    }
}

fn present(
    buffer: smithay_client_toolkit::shm::slot::Buffer,
    surface: &WlSurface,
    width: u32,
    height: u32,
) {
    if buffer.attach_to(surface).is_ok() {
        surface.damage_buffer(0, 0, width as i32, height as i32);
        surface.commit();
    }
}

fn evdev_to_ascii(key: u32, shift: bool) -> Option<char> {
    let ch = match key {
        2..=10 => (b'1' + (key - 2) as u8) as char,
        11 => '0',
        12 => if shift { '_' } else { '-' },
        16 => 'q',
        17 => 'w',
        18 => 'e',
        19 => 'r',
        20 => 't',
        21 => 'y',
        22 => 'u',
        23 => 'i',
        24 => 'o',
        25 => 'p',
        30 => 'a',
        31 => 's',
        32 => 'd',
        33 => 'f',
        34 => 'g',
        35 => 'h',
        36 => 'j',
        37 => 'k',
        38 => 'l',
        44 => 'z',
        45 => 'x',
        46 => 'c',
        47 => 'v',
        48 => 'b',
        49 => 'n',
        50 => 'm',
        52 => '.',
        53 => '/',
        57 => ' ',
        _ => return None,
    };
    Some(if shift { ch.to_ascii_uppercase() } else { ch })
}

// --------------------------------------------------------- Wayland traits

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
        if layer.wl_surface() == &self.bar.surface {
            std::process::exit(0);
        }
        if self.launcher.as_ref().is_some_and(|s| &s.layer == layer) {
            self.launcher = None;
        }
        if self.control.as_ref().is_some_and(|s| &s.layer == layer) {
            self.control = None;
        }
        if self.island_card.as_ref().is_some_and(|s| &s.layer == layer) {
            self.island_card = None;
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let apply = |slot: &mut SurfaceSlot| {
            if configure.new_size.0 > 0 {
                slot.width = configure.new_size.0;
            }
            if configure.new_size.1 > 0 {
                slot.height = configure.new_size.1;
            }
            slot.configured = true;
        };

        if &self.bar.layer == layer {
            apply(&mut self.bar);
        } else if &self.dock.layer == layer {
            apply(&mut self.dock);
        } else if let Some(ref mut s) = self.launcher {
            if &s.layer == layer {
                apply(s);
            }
        } else if let Some(ref mut s) = self.control {
            if &s.layer == layer {
                apply(s);
            }
        } else if let Some(ref mut s) = self.island_card {
            if &s.layer == layer {
                apply(s);
            }
        }
        // Check remaining overlays safely in case `else if` skipped them.
        if let Some(ref mut s) = self.control {
            if &s.layer == layer {
                apply(s);
            }
        }
        if let Some(ref mut s) = self.island_card {
            if &s.layer == layer {
                apply(s);
            }
        }
        self.needs_redraw = true;
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(p) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(p);
            }
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match &event.kind {
                PointerEventKind::Enter { .. } => {
                    self.pointer_surface = Some(event.surface.clone());
                    self.pointer_pos = event.position;
                    self.needs_redraw = true;
                }
                PointerEventKind::Leave { .. } => {
                    self.pointer_surface = None;
                    self.needs_redraw = true;
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_pos = event.position;
                    self.needs_redraw = true;
                }
                PointerEventKind::Press { button, .. } if *button == 0x110 => {
                    let surface = event.surface.clone();
                    let (x, y) = event.position;
                    self.handle_click(&surface, x, y);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlKeyboard, ()> for App {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: WEnum::Value(ks),
            ..
        } = event
        {
            state.handle_evdev_key(key, ks == KeyState::Pressed);
        }
    }
}

impl Dispatch<WlCallback, ()> for App {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: wayland_client::protocol::wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_callback::Event::Done { .. } = event {
            state.needs_redraw = true;
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_compositor!(App);
smithay_client_toolkit::delegate_shm!(App);
smithay_client_toolkit::delegate_output!(App);
smithay_client_toolkit::delegate_layer!(App);
smithay_client_toolkit::delegate_registry!(App);
smithay_client_toolkit::delegate_seat!(App);
smithay_client_toolkit::delegate_pointer!(App);

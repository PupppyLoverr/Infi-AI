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
//! * **Expanded Island** (`solyra-island`): activity popover.
//! * **Agent tasks** (`solyra-agents`): tasks and approval requests reported by
//!   an agent runtime through the file protocol in `docs/agent-protocol.md`.

mod agent;
mod canvas;
mod launcher;
mod notify;
mod state;
mod surfaces;
mod tokens;

#[cfg(test)]
mod preview;

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
    apply_wallpaper, catalog_apps, read_island_file, read_telemetry, send_client_cmd, send_ipc,
    spawn_ipc_listener, windows, workspaces, AppEntry, DockStyle, LaunchTarget, ShellCmd,
    ShellPrefs, WindowInfo,
};
use surfaces::{
    draw_agents, draw_bar, draw_control, draw_dock, draw_island_card, draw_launcher,
    floating_dock_width, AgentsView, HitAction, Live, Region, AGENTS_H, AGENTS_W, BAR_HEIGHT,
    CONTROL_H, CONTROL_W, DOCK_FLOAT_HEIGHT, ISLAND_CARD_H, ISLAND_CARD_W, LAUNCHER_H, LAUNCHER_W,
    TASKBAR_HEIGHT,
};
use tokens::Theme;
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
            "toggle-agents" | "agents" => Some("toggle-agents".into()),
            "close" => Some("close".into()),
            "dock" => args.get(2).map(|v| format!("dock {v}")),
            "wallpaper" => args.get(2).map(|v| format!("wallpaper {v}")),
            "wallpaper-file" => args
                .get(3)
                .map(|_| format!("wallpaper-file {} {}", args[2], args[3..].join(" "))),
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
        notify::spawn_server(cmd_queue.clone(), move || {
            conn_wake.display().sync(&qh_wake, ());
            let _ = conn_wake.flush();
        });
    }
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
        agents: None,
        prefs_mtime: ShellPrefs::config_mtime(),
        agent_tasks: Vec::new(),
        agent_summary: agent::Summary::default(),
        agent_attention: None,
        agents_selected: 0,
        agents_error: None,
        reply_buf: String::new(),
        reply_mode: false,
        agent_activity: None,
        catalog: Vec::new(),
        launcher_items: Vec::new(),
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

    // Re-apply the saved wallpaper: the session config only starts a default.
    if app.prefs.wallpaper_file.is_some() || app.prefs.wallpaper != state::WallpaperPreset::Graphite
    {
        app.apply_saved_wallpaper();
    }

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
    agents: Option<SurfaceSlot>,
    /// The config file's mtime as we last read or wrote it.
    prefs_mtime: Option<std::time::SystemTime>,

    agent_tasks: Vec<agent::TaskView>,
    agent_summary: agent::Summary,
    /// "Approval needed · <task>" while a task waits on the human.
    agent_attention: Option<String>,
    agents_selected: usize,
    agents_error: Option<String>,
    /// Text being typed to the selected agent (an answer or a message).
    reply_buf: String,
    /// The user pressed R: keys type into `reply_buf` instead of acting.
    reply_mode: bool,
    /// What a working agent is doing, for the island when nothing else is.
    agent_activity: Option<String>,

    /// Installed apps and commands, read when the launcher opens (not per key).
    catalog: Vec<AppEntry>,
    launcher_items: Vec<launcher::Item>,
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
    fn theme(&self) -> Theme {
        Theme::for_kind(self.prefs.theme)
    }

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

    fn close_overlays(&mut self) {
        self.launcher = None;
        self.control = None;
        self.island_card = None;
        self.agents = None;
        self.agents_error = None;
        self.reply_buf.clear();
        self.reply_mode = false;
    }

    fn open_overlay(
        &self,
        namespace: &str,
        anchor: Anchor,
        margin: (i32, i32, i32, i32),
        size: (u32, u32),
        kb: KeyboardInteractivity,
    ) -> SurfaceSlot {
        make_layer_surface(
            &self.compositor,
            &self.layer_shell,
            &self.qh,
            Layer::Overlay,
            namespace,
            anchor,
            0,
            margin,
            size.0,
            size.1,
            kb,
        )
    }

    fn apply_shell_cmd(&mut self, cmd: ShellCmd) {
        match cmd {
            ShellCmd::Notify(text) => {
                if text.trim().is_empty() {
                    self.island_override = None;
                } else if self.prefs.dnd_on {
                    // Focus is on: hold plain messages. Agent approvals come
                    // from the protocol, not from here, and are never muted.
                } else {
                    self.island_override = Some((Instant::now(), text));
                }
                self.island_card = None;
            }
            ShellCmd::ToggleLauncher => {
                if self.launcher.is_some() {
                    self.launcher = None;
                } else {
                    self.close_overlays();
                    self.catalog = catalog_apps();
                    self.launcher_query.clear();
                    self.launcher_selected = 0;
                    self.refresh_launcher_items();
                    self.launcher = Some(self.open_overlay(
                        "solyra-launcher",
                        Anchor::empty(),
                        (0, 0, 0, 0),
                        (LAUNCHER_W, LAUNCHER_H),
                        KeyboardInteractivity::Exclusive,
                    ));
                }
            }
            ShellCmd::ToggleControl => {
                if self.control.is_some() {
                    self.control = None;
                } else {
                    self.close_overlays();
                    self.control = Some(self.open_overlay(
                        "solyra-control",
                        Anchor::TOP | Anchor::RIGHT,
                        (8, 12, 0, 0),
                        (CONTROL_W, CONTROL_H),
                        KeyboardInteractivity::OnDemand,
                    ));
                }
            }
            ShellCmd::ToggleIsland => {
                if self.island_card.is_some() {
                    self.island_card = None;
                } else {
                    self.close_overlays();
                    self.island_card = Some(self.open_overlay(
                        "solyra-island",
                        Anchor::TOP,
                        (6, 0, 0, 0),
                        (ISLAND_CARD_W, ISLAND_CARD_H),
                        KeyboardInteractivity::OnDemand,
                    ));
                }
            }
            ShellCmd::ToggleAgents => {
                if self.agents.is_some() {
                    self.close_overlays();
                } else {
                    self.close_overlays();
                    self.load_agents();
                    // First task that needs a decision, else the first task.
                    self.agents_selected = self
                        .agent_tasks
                        .iter()
                        .position(|t| t.pending.is_some() && t.decision_sent.is_none())
                        .unwrap_or(0);
                    // Exclusive: the approval keys (A/D/T) must reach this
                    // card. It only opens when the user asks for it.
                    self.agents = Some(self.open_overlay(
                        "solyra-agents",
                        Anchor::TOP,
                        (6, 0, 0, 0),
                        (AGENTS_W, AGENTS_H),
                        KeyboardInteractivity::Exclusive,
                    ));
                }
            }
            ShellCmd::CloseOverlays => self.close_overlays(),
            ShellCmd::SetDock(style) => {
                self.prefs.dock_style = style;
                self.save_prefs();
                self.apply_dock_style(style);
            }
            ShellCmd::SetWallpaper(preset) => {
                self.prefs.wallpaper = preset;
                self.prefs.wallpaper_file = None;
                self.save_prefs();
                apply_wallpaper(preset);
            }
            ShellCmd::SetWallpaperFile { mode, path } => match state::validate_wallpaper(&path) {
                Ok(canon) => match state::apply_wallpaper_file(&canon, mode) {
                    Ok(()) => {
                        self.prefs.wallpaper_file = Some(canon.to_string_lossy().into_owned());
                        self.prefs.wallpaper_mode = mode;
                        self.save_prefs();
                    }
                    Err(err) => self.say(format!("Could not set wallpaper: swaybg: {err}")),
                },
                Err(err) => self.say(format!("Could not set wallpaper: {err}")),
            },
        }
        self.needs_state_sync = true;
        self.needs_redraw = true;
    }

    fn save_prefs(&mut self) {
        self.prefs.save();
        self.prefs_mtime = ShellPrefs::config_mtime();
    }

    /// Pick up edits another program (the Settings app) made to the config.
    fn reload_prefs_if_changed(&mut self) {
        let mtime = ShellPrefs::config_mtime();
        if mtime == self.prefs_mtime {
            return;
        }
        self.prefs_mtime = mtime;
        let new = ShellPrefs::load();
        let wallpaper_changed = new.wallpaper != self.prefs.wallpaper
            || new.wallpaper_file != self.prefs.wallpaper_file
            || new.wallpaper_mode != self.prefs.wallpaper_mode;
        let dock_changed = new.dock_style != self.prefs.dock_style;
        self.prefs = new;
        if wallpaper_changed {
            self.apply_saved_wallpaper();
        }
        if dock_changed {
            let style = self.prefs.dock_style;
            self.apply_dock_style(style);
        }
        self.needs_redraw = true;
    }

    fn apply_saved_wallpaper(&mut self) {
        let result = match self.prefs.wallpaper_file.clone() {
            Some(file) => state::validate_wallpaper(&file)
                .map_err(|e| e.to_string())
                .and_then(|p| {
                    state::apply_wallpaper_file(&p, self.prefs.wallpaper_mode)
                        .map_err(|e| e.to_string())
                }),
            None => {
                apply_wallpaper(self.prefs.wallpaper);
                Ok(())
            }
        };
        if let Err(err) = result {
            self.say(format!("Could not set wallpaper: {err}"));
        }
    }

    fn apply_dock_style(&mut self, style: DockStyle) {
        if self.dock_active_style != style {
            let wanted = floating_dock_width(style, &self.wins);
            self.dock =
                make_dock_surface(&self.compositor, &self.layer_shell, &self.qh, style, wanted);
            self.dock_active_style = style;
        }
    }

    fn refresh_launcher_items(&mut self) {
        self.launcher_items = launcher::search(
            &self.launcher_query,
            &self.catalog,
            &self.wins,
            self.agent_summary.runtime_present,
        );
        self.launcher_selected = self
            .launcher_selected
            .min(self.launcher_items.len().saturating_sub(1));
    }

    fn load_agents(&mut self) {
        let now = agent::now_secs();
        let Some(root) = agent::root_dir() else {
            self.agent_tasks.clear();
            self.agent_summary = agent::Summary::default();
            self.agent_attention = None;
            self.agent_activity = None;
            return;
        };
        let snap = agent::load(&root, now);
        for err in &snap.read_errors {
            eprintln!("solyra-panel: agent protocol: {err}");
        }
        self.agent_summary = agent::Summary::of(snap.runtime_present, &snap.tasks);
        self.agent_attention = snap
            .tasks
            .iter()
            .find(|t| t.pending.is_some() && t.decision_sent.is_none())
            .map(|t| match t.pending.as_ref().map(|p| p.kind) {
                Some(agent::AskKind::Question) => {
                    format!("Question · {}", t.agent.as_deref().unwrap_or(&t.name))
                }
                _ => format!("Approval needed · {}", t.name),
            });
        self.agent_activity = snap
            .tasks
            .iter()
            .find(|t| t.status == agent::Status::Running && !t.is_quiet(now))
            .map(|t| {
                let who = t.agent.as_deref().unwrap_or(&t.name);
                match t.latest_milestone() {
                    Some(m) => format!("{who} · {}", m.text),
                    None => format!("{who} · working"),
                }
            });
        self.agent_tasks = snap.tasks;
        self.agents_selected = self
            .agents_selected
            .min(self.agent_tasks.len().saturating_sub(1));
    }

    fn sync_compositor_state(&mut self) {
        self.wins = windows();
        self.ws_list = workspaces();
        self.reload_prefs_if_changed();
        self.load_agents();

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
        if self.prefs.dnd_on {
            self.island_msg = None;
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
        let th = self.theme();
        let agents_summary = self.agent_summary;

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
                    &th,
                    &self.fonts,
                    &self.ws_list,
                    &self.wins,
                    self.island_msg.as_deref(),
                    self.agent_attention.as_deref(),
                    self.agent_activity.as_deref(),
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
                let live = Live {
                    agents: agents_summary,
                    telem: &telem,
                    prefs: &self.prefs,
                };
                self.dock.regions = draw_dock(
                    &mut cv,
                    &th,
                    &self.fonts,
                    self.dock_active_style,
                    &self.wins,
                    &mut self.icons,
                    hover,
                    &live,
                );
                present(buffer, &self.dock.surface, w, h);
            }
        }

        // Overlays: each draws into its own shm buffer.
        macro_rules! overlay {
            ($slot:expr, |$cv:ident, $hover:ident| $draw:expr) => {
                if let Some(ref mut slot) = $slot {
                    if slot.configured && slot.width > 0 {
                        let $hover = self
                            .pointer_surface
                            .as_ref()
                            .filter(|s| *s == &slot.surface)
                            .map(|_| self.pointer_pos);
                        let (w, h) = (slot.width, slot.height);
                        if let Ok((buffer, bytes)) = self.pool.create_buffer(
                            w as i32,
                            h as i32,
                            (w * 4) as i32,
                            Format::Argb8888,
                        ) {
                            let mut $cv = Canvas::new(bytes, w, h);
                            slot.regions = $draw;
                            present(buffer, &slot.surface, w, h);
                        }
                    }
                }
            };
        }

        overlay!(self.launcher, |cv, hover| draw_launcher(
            &mut cv,
            &th,
            &self.fonts,
            &self.launcher_query,
            &self.launcher_items,
            self.launcher_selected,
            &mut self.icons,
            hover,
        ));
        overlay!(self.control, |cv, hover| draw_control(
            &mut cv,
            &th,
            &self.fonts,
            &self.prefs,
            &telem,
            agents_summary,
            hover,
        ));
        overlay!(self.island_card, |cv, _hover| draw_island_card(
            &mut cv,
            &th,
            &self.fonts,
            self.island_msg.as_deref(),
            &self.wins,
            &telem,
            agents_summary,
        ));
        let root_hint = agent::root_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "$XDG_RUNTIME_DIR/solyra-agent (XDG_RUNTIME_DIR is not set)".into());
        let view = AgentsView {
            runtime_present: agents_summary.runtime_present,
            tasks: &self.agent_tasks,
            selected: self.agents_selected,
            now: agent::now_secs(),
            root_hint: &root_hint,
            error: self.agents_error.as_deref(),
            reply: &self.reply_buf,
            typing: self.reply_mode || self.typing_question(),
        };
        overlay!(self.agents, |cv, hover| draw_agents(
            &mut cv,
            &th,
            &self.fonts,
            &view,
            hover
        ));

        let _ = conn.flush();
    }

    fn hover_on(&self, surface: &WlSurface) -> Option<(f64, f64)> {
        if self.pointer_surface.as_ref() == Some(surface) {
            Some(self.pointer_pos)
        } else {
            None
        }
    }

    fn regions_for(&self, surface: &WlSurface) -> Option<&Vec<Region>> {
        if surface == &self.bar.surface {
            return Some(&self.bar.regions);
        }
        if surface == &self.dock.surface {
            return Some(&self.dock.regions);
        }
        [
            &self.launcher,
            &self.control,
            &self.island_card,
            &self.agents,
        ]
        .into_iter()
        .flatten()
        .find(|s| &s.surface == surface)
        .map(|s| &s.regions)
    }

    fn handle_click(&mut self, surface: &WlSurface, x: f64, y: f64) {
        let Some(regions) = self.regions_for(surface) else {
            return;
        };
        let Some(hit) = regions
            .iter()
            .find(|r| r.contains(x, y))
            .map(|r| r.action.clone())
        else {
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
                send_ipc(&format!(
                    "{{\"Action\":{{\"FocusWindow\":{{\"id\":{id}}}}}}}"
                ));
                self.launcher = None;
            }
            HitAction::Launch(target) => {
                self.execute_launch(target);
                self.launcher = None;
            }
            HitAction::AgentRequest(text) => {
                self.queue_agent_request(&text);
                self.launcher = None;
            }
            HitAction::Shell(cmd) => {
                self.apply_shell_cmd(cmd);
            }
            HitAction::ToggleDnd => {
                self.prefs.dnd_on = !self.prefs.dnd_on;
                self.save_prefs();
            }
            HitAction::ToggleTheme => {
                self.prefs.theme = match self.prefs.theme {
                    tokens::ThemeKind::Dark => tokens::ThemeKind::Light,
                    tokens::ThemeKind::Light => tokens::ThemeKind::Dark,
                };
                self.save_prefs();
            }
            HitAction::SelectTask(idx) => {
                self.agents_selected = idx.min(self.agent_tasks.len().saturating_sub(1));
                self.agents_error = None;
            }
            HitAction::Choose { task, choice } => {
                self.agents_selected = task;
                let text = self
                    .agent_tasks
                    .get(task)
                    .and_then(|t| t.pending.as_ref())
                    .and_then(|p| p.choices.get(choice).cloned());
                if let Some(text) = text {
                    self.answer(&text);
                }
            }
            HitAction::StartReply => {
                if self
                    .agent_tasks
                    .get(self.agents_selected)
                    .is_some_and(|t| t.interactive)
                {
                    self.reply_mode = true;
                }
            }
            HitAction::Decide {
                task,
                verdict,
                scope,
            } => self.decide(task, verdict, scope),
            HitAction::SetVolumeFromX { x0, w } => {
                let pct = (((x - x0 as f64) / w.max(1) as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8;
                self.prefs.volume = pct;
                self.save_prefs();
                let _ = std::process::Command::new("wpctl")
                    .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{pct}%")])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            HitAction::SetBrightnessFromX { x0, w } => {
                let pct = (((x - x0 as f64) / w.max(1) as f64) * 100.0)
                    .round()
                    .clamp(15.0, 100.0) as u8;
                self.prefs.brightness = pct;
                self.save_prefs();
                let _ = std::process::Command::new("brightnessctl")
                    .args(["set", &format!("{pct}%")])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        }
        self.needs_state_sync = true;
        self.needs_redraw = true;
    }

    /// Say something in the island, even while Focus is on: this is the
    /// direct result of something the user just did.
    fn say(&mut self, text: String) {
        self.island_override = Some((Instant::now(), text));
    }

    fn queue_agent_request(&mut self, text: &str) {
        let Some(root) = agent::root_dir() else {
            self.say("Could not queue request: XDG_RUNTIME_DIR is not set".into());
            return;
        };
        match agent::submit_request(&root, text, agent::now_secs()) {
            Ok(_) => self.say("Request queued for the agent runtime".into()),
            Err(err) => self.say(format!("Could not queue request: {err}")),
        }
    }

    fn decide(&mut self, task: usize, verdict: agent::Verdict, scope: agent::Scope) {
        let Some(t) = self.agent_tasks.get(task) else {
            return;
        };
        let Some(pending) = t.pending.as_ref().filter(|_| t.decision_sent.is_none()) else {
            return;
        };
        let (task_id, request_id) = (t.id.clone(), pending.request_id.clone());
        let Some(root) = agent::root_dir() else {
            self.agents_error = Some("Decision not recorded: XDG_RUNTIME_DIR is not set".into());
            return;
        };
        match agent::record_decision(
            &root,
            &task_id,
            &request_id,
            verdict,
            scope,
            agent::now_secs(),
        ) {
            Ok(()) => {
                self.agents_error = None;
                self.load_agents();
            }
            Err(err) => {
                self.agents_error = Some(format!("Decision not recorded: {err}"));
            }
        }
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

    fn run_launcher_item(&mut self, item: launcher::Item) {
        match item.action {
            launcher::Action::Launch(target) => {
                self.launcher = None;
                self.execute_launch(target);
            }
            launcher::Action::FocusWindow(id) => {
                self.launcher = None;
                send_ipc(&format!(
                    "{{\"Action\":{{\"FocusWindow\":{{\"id\":{id}}}}}}}"
                ));
            }
            launcher::Action::AgentRequest(text) => {
                self.launcher = None;
                self.queue_agent_request(&text);
            }
            // Stay open: the row explains why nothing happened.
            launcher::Action::Unavailable => {}
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
        // Escape dismisses the topmost overlay and nothing else: it never
        // answers an approval.
        if key == 1 {
            if self.agents.is_some() && self.reply_mode {
                // First Esc only leaves the text field; the card stays.
                self.reply_mode = false;
                self.reply_buf.clear();
                self.needs_redraw = true;
                return;
            }
            self.apply_shell_cmd(ShellCmd::CloseOverlays);
            return;
        }
        if self.agents.is_some() {
            self.handle_agents_key(key);
            self.needs_redraw = true;
            return;
        }
        if self.launcher.is_none() {
            return;
        }

        let max_idx = self.launcher_items.len().saturating_sub(1);
        match key {
            14 => {
                self.launcher_query.pop();
                self.launcher_selected = 0;
                self.refresh_launcher_items();
            }
            28 | 96 => {
                // Enter runs exactly the highlighted row. With no rows there
                // is nothing to run: a typed query is never executed as a
                // shell command.
                if let Some(item) = self
                    .launcher_items
                    .get(self.launcher_selected.min(max_idx))
                    .cloned()
                {
                    self.run_launcher_item(item);
                }
            }
            103 | 105 => {
                self.launcher_selected = self.launcher_selected.saturating_sub(1);
            }
            108 | 106 | 15 => {
                self.launcher_selected = (self.launcher_selected + 1).min(max_idx);
            }
            _ => {
                if let Some(ch) = evdev_to_ascii(key, self.shift_held) {
                    self.launcher_query.push(ch);
                    self.launcher_selected = 0;
                    self.refresh_launcher_items();
                }
            }
        }
        self.needs_redraw = true;
    }

    /// The selected task's open question that takes typed text (no fixed choices).
    fn typing_question(&self) -> bool {
        self.agent_tasks
            .get(self.agents_selected)
            .and_then(|t| t.pending.as_ref().filter(|_| t.decision_sent.is_none()))
            .is_some_and(|p| p.kind == agent::AskKind::Question && p.choices.is_empty())
    }

    fn handle_agents_key(&mut self, key: u32) {
        let last = self.agent_tasks.len().saturating_sub(1);
        // Typing: an answer to a question, or a message to an interactive agent.
        if self.reply_mode || self.typing_question() {
            match key {
                14 => {
                    self.reply_buf.pop();
                }
                28 | 96 => self.send_reply(),
                103 | 108 if !self.reply_mode => {
                    // Arrows still move between tasks while a question is open.
                    self.move_selection(key == 108, last);
                }
                _ => {
                    if let Some(ch) = evdev_to_ascii(key, self.shift_held) {
                        if self.reply_buf.chars().count() < agent::MAX_REPLY_CHARS {
                            self.reply_buf.push(ch);
                        }
                    }
                }
            }
            return;
        }
        match key {
            103 => self.move_selection(false, last),
            108 => self.move_selection(true, last),
            // R: message the selected agent (only if it can receive text).
            19 => {
                let can = self
                    .agent_tasks
                    .get(self.agents_selected)
                    .is_some_and(|t| t.interactive && !t.status.is_terminal());
                if can {
                    self.reply_mode = true;
                    self.reply_buf.clear();
                    self.agents_error = None;
                } else {
                    self.agents_error =
                        Some("This agent cannot receive messages from here.".into());
                }
            }
            // 1-8 pick an offered choice for a question.
            2..=9 => {
                let n = (key - 2) as usize;
                let choice = self
                    .agent_tasks
                    .get(self.agents_selected)
                    .and_then(|t| t.pending.as_ref().filter(|_| t.decision_sent.is_none()))
                    .filter(|p| p.kind == agent::AskKind::Question)
                    .and_then(|p| p.choices.get(n).cloned());
                if let Some(text) = choice {
                    self.answer(&text);
                }
            }
            // A / D / T. Enter is deliberately unbound: an approval needs a
            // deliberate key, not the one that also dismisses dialogs.
            30 => self.decide(
                self.agents_selected,
                agent::Verdict::Approve,
                agent::Scope::Once,
            ),
            20 => self.decide(
                self.agents_selected,
                agent::Verdict::Approve,
                agent::Scope::Task,
            ),
            32 => self.decide(
                self.agents_selected,
                agent::Verdict::Deny,
                agent::Scope::Once,
            ),
            _ => {}
        }
    }

    fn move_selection(&mut self, down: bool, last: usize) {
        self.agents_selected = if down {
            (self.agents_selected + 1).min(last)
        } else {
            self.agents_selected.saturating_sub(1)
        };
        self.agents_error = None;
        self.reply_buf.clear();
        self.reply_mode = false;
    }

    /// Enter in the reply field: answer the open question, else message the agent.
    fn send_reply(&mut self) {
        let text = std::mem::take(&mut self.reply_buf);
        if self.typing_question() {
            self.answer(&text);
        } else if let Some(t) = self.agent_tasks.get(self.agents_selected) {
            let id = t.id.clone();
            let Some(root) = agent::root_dir() else {
                self.agents_error = Some("Message not sent: XDG_RUNTIME_DIR is not set".into());
                return;
            };
            match agent::send_message(&root, &id, &text, agent::now_secs()) {
                Ok(()) => {
                    self.agents_error = None;
                    self.reply_mode = false;
                }
                Err(err) => {
                    self.reply_buf = text;
                    self.agents_error = Some(format!("Message not sent: {err}"));
                }
            }
        }
    }

    fn answer(&mut self, text: &str) {
        let Some(t) = self.agent_tasks.get(self.agents_selected) else {
            return;
        };
        let Some(p) = t.pending.as_ref().filter(|_| t.decision_sent.is_none()) else {
            return;
        };
        let (task_id, request_id) = (t.id.clone(), p.request_id.clone());
        let Some(root) = agent::root_dir() else {
            self.agents_error = Some("Answer not sent: XDG_RUNTIME_DIR is not set".into());
            return;
        };
        match agent::record_answer(&root, &task_id, &request_id, text, agent::now_secs()) {
            Ok(()) => {
                self.agents_error = None;
                self.reply_buf.clear();
                self.reply_mode = false;
                self.load_agents();
            }
            Err(err) => {
                self.reply_buf = text.to_string();
                self.agents_error = Some(format!("Answer not sent: {err}"));
            }
        }
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
        12 => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
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
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {}
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if layer.wl_surface() == &self.bar.surface {
            std::process::exit(0);
        }
        for slot in [
            &mut self.launcher,
            &mut self.control,
            &mut self.island_card,
            &mut self.agents,
        ] {
            if slot.as_ref().is_some_and(|s| &s.layer == layer) {
                *slot = None;
            }
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
        }
        for slot in [
            &mut self.launcher,
            &mut self.control,
            &mut self.island_card,
            &mut self.agents,
        ] {
            if let Some(s) = slot.as_mut().filter(|s| &s.layer == layer) {
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

//! Solyra shell (`solyra-panel`) — unified Wayland desktop shell:
//!
//! * **Top Bar** (`solyra-bar`): translucent glass menu bar with Solyra mark,
//!   focused app & contextual menus, workspace pills, Apple-style notch +
//!   Dynamic Island, Search button, Status capsule, and clock.
//! * **Dock** (`solyra-dock` / `solyra-taskbar`): all 3 variants live-switchable
//!   (`solyra` hybrid default, `macos` Sequoia dock, `windows` Fluent taskbar).
//! * **Launcher** (`solyra-launcher`): Spotlight. A "Search or Ask" capsule that
//!   opens a results card (Top Hit, Applications, windows) once there is a query.
//! * **Control Centre** (`solyra-control`): quick toggles, sliders, live Dock
//!   variant switcher, live Wallpaper switcher, and memory telemetry.
//! * **Expanded Island** (`solyra-island`): activity popover.
//! * **Agent tasks** (`solyra-agents`): tasks and approval requests reported by
//!   an agent runtime through the file protocol in `docs/agent-protocol.md`.

mod agent;
mod canvas;
mod desk;
mod dock;
mod icons;
mod island;
mod launcher;
mod lock;
mod material;
mod media;
mod menu;
mod notify;
mod pam;
mod state;
mod surfaces;
mod tokens;

#[cfg(test)]
mod preview;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    spawn_ipc_listener, windows, workspaces, AppEntry, LaunchTarget, ShellCmd, ShellPrefs,
    WindowInfo,
};
use surfaces::{
    draw_about, draw_agents, draw_bar, draw_control, draw_island_card, draw_launcher,
    draw_notifs, spot_layout, AgentsView,
    HitAction, Live, Region, AGENTS_H, AGENTS_W, BAR_HEIGHT, CONTROL_H, CONTROL_W, ISLAND_CARD_H,
    ISLAND_CARD_W,
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
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::{self, ExtBackgroundEffectSurfaceV1},
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("lock") {
        if let Err(err) = lock::run() {
            eprintln!("solyra-panel lock: {err}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(sub) = args.get(1).map(String::as_str) {
        let cmd_line = match sub {
            "notify" => Some(format!("notify {}", args[2..].join(" "))),
            "toggle-launcher" | "launcher" => Some("toggle-launcher".into()),
            "toggle-control" | "control" => Some("toggle-control".into()),
            "toggle-island" | "island" => Some("toggle-island".into()),
            "about" => Some("about".into()),
            "toggle-notifs" | "notifs" => Some("toggle-notifs".into()),
            "clear-notifs" => Some("clear-notifs".into()),
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

    let mut prefs = ShellPrefs::load();
    prefs.compute_wall_tint();
    let blur_mgr = globals
        .bind::<ExtBackgroundEffectManagerV1, _, _>(&qh, 1..=1, ())
        .ok();
    let bar_material = prefs.look.material(prefs.look.bar_material);
    let dock_material = prefs.look.material(prefs.look.dock_material);
    let bar = make_layer_surface(
        &compositor,
        &layer_shell,
        &qh,
        Layer::Top,
        &material::namespace("solyra-bar", bar_material),
        Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        BAR_HEIGHT as i32,
        (0, 0, 0, 0),
        0,
        BAR_HEIGHT,
        KeyboardInteractivity::None,
    );
    let dock_hidden = prefs.look.dock_autohide;
    let dock_spec = dock::spec(prefs.dock_style, &[], prefs.look.dock_magnify, dock_hidden);
    let dock = make_dock_surface(&compositor, &layer_shell, &qh, &dock_spec, dock_material);

    let waker: Arc<dyn Fn() + Send + Sync> = {
        let (conn_wake, qh_wake) = (conn.clone(), qh.clone());
        Arc::new(move || {
            conn_wake.display().sync(&qh_wake, ());
            let _ = conn_wake.flush();
        })
    };
    let cmd_queue: Arc<Mutex<Vec<ShellCmd>>> = Arc::new(Mutex::new(Vec::new()));
    let media_slot: Arc<Mutex<Option<media::Media>>> = Arc::default();
    {
        let w = waker.clone();
        media::spawn_watcher(media_slot.clone(), move || w());
    }
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
        blur_mgr,
        bar_material,
        dock_material,
        notch_material: material::Material::Solid,
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
        dock_spec,
        dock_hidden,
        dock_left_at: None,
        waker: waker.clone(),
        launcher: None,
        control: None,
        island_card: None,
        notifs_surface: None,
        dock_mouse_smooth: None,
        launching: Vec::new(),
        dock_ctx: None,
        dock_ctx_entries: Vec::new(),
        about: None,
        notifs: std::collections::VecDeque::new(),
        agents: None,
        desk: None,
        desk_material: material::Material::Solid,
        menu: None,
        menu_open: None,
        menu_armed: None,
        island: None,
        island_anim: island::Anim::new(island::target(
            solyra_conf::look::NotchStyle::Notch,
            &island::Content::Idle,
            false,
        )),
        island_expanded: false,
        island_last_frame: Instant::now(),
        island_frame_only: false,
        island_left_at: None,
        media: media_slot,
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

    app.ensure_island();
    app.ensure_bar();
    app.ensure_desk();

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
            if !app.island_frame_only
                && (app.needs_state_sync || app.last_sync.elapsed() >= Duration::from_millis(800))
            {
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
    /// Blur-behind request (ext-background-effect), made on first use.
    effect: Option<ExtBackgroundEffectSurfaceV1>,
    /// The blur region last sent, to skip resending an unchanged one.
    blur: Vec<(i32, i32, i32, i32)>,
}

impl Drop for SurfaceSlot {
    fn drop(&mut self) {
        if let Some(e) = self.effect.take() {
            e.destroy();
        }
    }
}

/// Ask the compositor to blur behind exactly `rects` of this surface (empty:
/// no blur). Takes effect with the surface's next commit. Without the
/// protocol (another compositor) materials stay plain tints.
fn set_blur(
    mgr: Option<&ExtBackgroundEffectManagerV1>,
    compositor: &CompositorState,
    qh: &QueueHandle<App>,
    slot: &mut SurfaceSlot,
    rects: Vec<(i32, i32, i32, i32)>,
) {
    let Some(mgr) = mgr else {
        return;
    };
    if rects == slot.blur && slot.effect.is_some() {
        return;
    }
    let effect = slot
        .effect
        .get_or_insert_with(|| mgr.get_background_effect(&slot.surface, qh, ()));
    if rects.is_empty() {
        effect.set_blur_region(None);
    } else if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor) {
        for &(x, y, w, h) in &rects {
            region.add(x, y, w, h);
        }
        effect.set_blur_region(Some(region.wl_region()));
    }
    slot.blur = rects;
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
        effect: None,
        blur: Vec::new(),
    }
}

fn make_dock_surface(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<App>,
    spec: &dock::Spec,
    mat: material::Material,
) -> SurfaceSlot {
    let anchor = if spec.full_width {
        Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT
    } else {
        Anchor::BOTTOM
    };
    let slot = make_layer_surface(
        compositor,
        layer_shell,
        qh,
        Layer::Top,
        &material::namespace(spec.namespace, mat),
        anchor,
        spec.exclusive,
        (0, 0, spec.bottom_margin, 0),
        spec.width,
        spec.height,
        KeyboardInteractivity::None,
    );
    set_dock_input_region(compositor, &slot.surface, spec);
    slot
}

/// A magnifying dock's surface is taller than its card; only the card takes
/// the pointer, so clicks in the clear room above reach the window below.
fn set_dock_input_region(compositor: &CompositorState, surface: &WlSurface, spec: &dock::Spec) {
    if spec.namespace != "solyra-dock-magnify" {
        surface.set_input_region(None);
        return;
    }
    if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor) {
        let card_top = spec.height as i32 - dock::CARD_H;
        region.add(0, card_top, spec.width as i32, dock::CARD_H);
        surface.set_input_region(Some(region.wl_region()));
    }
}

struct App {
    blur_mgr: Option<ExtBackgroundEffectManagerV1>,
    /// Materials the current bar/dock/notch surfaces were made with (their
    /// namespaces carry it, so a change recreates the surface).
    bar_material: material::Material,
    dock_material: material::Material,
    notch_material: material::Material,
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
    dock_spec: dock::Spec,
    /// Auto-hide has tucked the dock away.
    dock_hidden: bool,
    /// When the pointer last left the dock (auto-hide waits a moment).
    dock_left_at: Option<Instant>,
    /// Wakes the event loop from another thread.
    waker: Arc<dyn Fn() + Send + Sync>,
    launcher: Option<SurfaceSlot>,
    /// Notification centre drawer (right edge).
    notifs_surface: Option<SurfaceSlot>,
    /// Smoothed pointer x over the dock (pearOS lerp), drives magnification.
    dock_mouse_smooth: Option<f32>,
    /// Apps clicked open from the dock: icons bounce while they start.
    launching: Vec<(String, Instant)>,
    /// Dock icon context menu (right-click).
    dock_ctx: Option<SurfaceSlot>,
    dock_ctx_entries: Vec<menu::Entry>,
    /// "About This Computer" card.
    about: Option<SurfaceSlot>,
    control: Option<SurfaceSlot>,
    island_card: Option<SurfaceSlot>,
    agents: Option<SurfaceSlot>,
    /// Right-side desk card when desktop mode is Agent workspace.
    desk: Option<SurfaceSlot>,
    desk_material: material::Material,
    /// The notch / Dynamic Island surface (absent when the style is Off).
    /// An open menu-bar menu: a full-screen transparent overlay.
    menu: Option<SurfaceSlot>,
    menu_open: Option<menu::MenuId>,
    /// An item waiting for its confirming second click.
    menu_armed: Option<usize>,
    island: Option<SurfaceSlot>,
    island_anim: island::Anim,
    island_expanded: bool,
    island_last_frame: Instant,
    /// The next redraw is only an island animation frame.
    island_frame_only: bool,
    island_left_at: Option<Instant>,
    media: Arc<Mutex<Option<media::Media>>>,
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
    /// Notification history for the centre: (unix secs, app, text).
    notifs: std::collections::VecDeque<(u64, String, String)>,
    icons: HashMap<String, Option<Icon>>,
    fonts: Fonts,
    needs_redraw: bool,
    needs_state_sync: bool,
    last_sync: Instant,
}

impl App {
    fn theme(&self) -> Theme {
        let accent =
            tokens::Accent::parse(&self.prefs.look.accent).unwrap_or(tokens::Accent::Blue);
        let t = Theme::with(self.prefs.theme, accent);
        if self.prefs.look.opaque {
            t.opaque()
        } else {
            t
        }
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
        self.notifs_surface = None;
        self.dock_ctx = None;
        self.dock_ctx_entries.clear();
        self.about = None;
        self.island_card = None;
        self.agents = None;
        self.agents_error = None;
        self.menu = None;
        self.menu_open = None;
        self.menu_armed = None;
        self.reply_buf.clear();
        self.reply_mode = false;
        if self.island_expanded {
            self.island_expanded = false;
            self.retarget_island();
        }
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
            ShellCmd::Notify(app, text) => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    self.island_override = None;
                } else {
                    // The centre keeps history even when Focus holds the
                    // banner back, like macOS Notification Centre.
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if self.notifs.back() != Some(&(ts, app.clone(), text.clone())) {
                        self.notifs.push_back((ts, app.clone(), text.clone()));
                        while self.notifs.len() > 30 {
                            self.notifs.pop_front();
                        }
                    }
                    if !self.prefs.dnd_on {
                        self.island_override = Some((Instant::now(), text));
                    }
                }
                self.island_card = None;
            }
            ShellCmd::ToggleNotifs => {
                if self.notifs_surface.is_some() {
                    self.notifs_surface = None;
                } else {
                    self.close_overlays();
                    // Full-height right-edge drawer. Height is passed
                    // explicitly: anchored to top+bottom niri reports 0 as
                    // the configured size, and a 0-height buffer never draws.
                    let out_h = self
                        .output_state
                        .outputs()
                        .filter_map(|o| self.output_state.info(&o))
                        .filter_map(|i| i.logical_size.map(|(_, h)| h))
                        .max()
                        .unwrap_or(768);
                    self.notifs_surface = Some(self.open_overlay(
                        "solyra-notifs",
                        Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM,
                        (8, 10, 8, 0),
                        (340, (out_h - 16).max(1) as u32),
                        KeyboardInteractivity::OnDemand,
                    ));
                }
            }
            ShellCmd::ClearNotifs => {
                self.notifs.clear();
                self.needs_redraw = true;
            }
            ShellCmd::About => {
                if self.about.is_some() {
                    self.about = None;
                } else {
                    self.close_overlays();
                    self.about = Some(self.open_overlay(
                        "solyra-about",
                        Anchor::TOP,
                        (140, 0, 0, 0),
                        (340, 210),
                        KeyboardInteractivity::OnDemand,
                    ));
                }
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
                    let lay = spot_layout(&self.launcher_query, &self.launcher_items);
                    self.launcher = Some(self.open_overlay(
                        "solyra-launcher",
                        Anchor::TOP,
                        (self.spotlight_top(), 0, 0, 0),
                        (lay.w, lay.h),
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
            ShellCmd::ToggleIsland if self.island.is_some() => {
                self.island_expanded = !self.island_expanded;
                self.island_left_at = None;
                if !self.island_expanded {
                    self.reply_mode = false;
                    self.reply_buf.clear();
                }
                self.retarget_island();
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
                self.ensure_dock();
            }
            ShellCmd::Lock => {
                self.close_overlays();
                spawn_lock();
            }
            ShellCmd::ToggleDockAutohide => {
                let on = !self.prefs.look.dock_autohide;
                self.prefs.look.dock_autohide = on;
                self.dock_hidden = on;
                if let Some(path) = solyra_conf::config_path("shell.conf") {
                    let _ = solyra_conf::update(
                        &path,
                        &[("dock_autohide", Some(if on { "1" } else { "0" }.into()))],
                    );
                    self.prefs_mtime = ShellPrefs::config_mtime();
                }
                self.ensure_dock();
            }
            ShellCmd::RevealDock => {
                self.dock_hidden = false;
                self.dock_left_at = None;
                self.ensure_dock();
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

    /// The largest integer scale among the connected outputs (at least 1).
    /// One shared scale keeps every surface sharp on the busiest display; on
    /// a mixed setup a lower-scale display shows it downscaled by the
    /// compositor. Fractional scales are rounded up by the compositor and
    /// arrive here as the next integer.
    fn output_scale(&self) -> u32 {
        self.output_state
            .outputs()
            .filter_map(|o| self.output_state.info(&o))
            .map(|i| i.scale_factor)
            .max()
            .unwrap_or(1)
            .clamp(1, 4) as u32
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
        let mut new = ShellPrefs::load();
        let wallpaper_changed = new.wallpaper != self.prefs.wallpaper
            || new.wallpaper_file != self.prefs.wallpaper_file
            || new.wallpaper_mode != self.prefs.wallpaper_mode;
        if wallpaper_changed {
            new.compute_wall_tint();
        } else {
            new.wall_tint = self.prefs.wall_tint;
        }
        self.prefs = new;
        self.ensure_island();
        self.ensure_bar();
        if wallpaper_changed {
            self.apply_saved_wallpaper();
        }
        self.ensure_dock();
        self.ensure_desk();
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

    /// Make the dock surface match the current style, magnification,
    /// auto-hide state and app count: resize it, or recreate it when the
    /// kind of surface changes.
    fn ensure_dock(&mut self) {
        let autohide = self.prefs.look.dock_autohide;
        if !autohide {
            self.dock_hidden = false;
        }
        let spec = dock::spec(
            self.prefs.dock_style,
            &self.wins,
            self.prefs.look.dock_magnify,
            self.dock_hidden && autohide,
        );
        let mat = self.prefs.look.material(self.prefs.look.dock_material);
        if spec == self.dock_spec && mat == self.dock_material {
            return;
        }
        let same_kind = mat == self.dock_material
            && spec.namespace == self.dock_spec.namespace
            && spec.full_width == self.dock_spec.full_width
            && spec.exclusive == self.dock_spec.exclusive
            && spec.bottom_margin == self.dock_spec.bottom_margin;
        if same_kind {
            self.dock.layer.set_size(spec.width, spec.height);
            self.dock.width = if spec.full_width {
                self.dock.width
            } else {
                spec.width
            };
            self.dock.height = spec.height;
            set_dock_input_region(&self.compositor, &self.dock.surface, &spec);
            self.dock.layer.commit();
        } else {
            self.dock =
                make_dock_surface(&self.compositor, &self.layer_shell, &self.qh, &spec, mat);
            self.dock_material = mat;
        }
        self.dock_spec = spec;
        self.needs_redraw = true;
    }

    /// Auto-hide: tuck the dock away a moment after the pointer leaves it.
    fn update_dock_autohide(&mut self) {
        if !self.prefs.look.dock_autohide || self.dock_hidden {
            return;
        }
        let over = self.pointer_surface.as_ref() == Some(&self.dock.surface);
        let menus_open = self.launcher.is_some() || self.control.is_some();
        if !over && !menus_open {
            if let Some(t) = self.dock_left_at {
                if t.elapsed() >= Duration::from_millis(700) {
                    self.dock_hidden = true;
                    self.dock_left_at = None;
                    self.ensure_dock();
                }
            }
        }
    }

    /// Space above the Spotlight capsule: it sits about a seventh of the way
    /// down the screen, like macOS. The output's height is not known here, so
    /// it is taken from the bar's width at 16:9.
    fn spotlight_top(&self) -> i32 {
        let screen_h = self.bar.width.max(1024) as f32 * 9.0 / 16.0;
        ((screen_h * 0.14) as i32 - 20).max(32)
    }

    /// Fit the Spotlight surface to the capsule and card it is showing, and
    /// limit input and blur to those two shapes.
    fn sync_launcher(&mut self) {
        let lay = spot_layout(&self.launcher_query, &self.launcher_items);
        let Some(slot) = self.launcher.as_mut() else {
            return;
        };
        if (slot.width, slot.height) != (lay.w, lay.h) {
            slot.layer.set_size(lay.w, lay.h);
            slot.width = lay.w;
            slot.height = lay.h;
        }
        let rects: Vec<(i32, i32, i32, i32)> = lay
            .shapes()
            .iter()
            .flat_map(|(r, radius)| material::blur_rects(r.x, r.y, r.w, r.h, *radius))
            .collect();
        if rects == slot.blur {
            return;
        }
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(&self.compositor) {
            for (r, _) in lay.shapes() {
                region.add(r.x, r.y, r.w, r.h);
            }
            slot.surface.set_input_region(Some(region.wl_region()));
        }
        set_blur(self.blur_mgr.as_ref(), &self.compositor, &self.qh, slot, rects);
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

    /// Create or remove the island surface to match the notch style.
    /// Hidden bar: a 1 px transparent strip that reserves nothing.
    fn ensure_bar(&mut self) {
        let hidden = self.prefs.look.bar == solyra_conf::look::BarStyle::Hidden;
        let (zone, h) = if hidden {
            (0, 1)
        } else {
            (BAR_HEIGHT as i32, BAR_HEIGHT)
        };
        let mat = self.prefs.look.material(self.prefs.look.bar_material);
        if mat != self.bar_material {
            self.bar = make_layer_surface(
                &self.compositor,
                &self.layer_shell,
                &self.qh,
                Layer::Top,
                &material::namespace("solyra-bar", mat),
                Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
                zone,
                (0, 0, 0, 0),
                0,
                h,
                KeyboardInteractivity::None,
            );
            self.bar_material = mat;
            self.needs_redraw = true;
            return;
        }
        if self.bar.height != h {
            self.bar.layer.set_exclusive_zone(zone);
            self.bar.layer.set_size(0, h);
            self.bar.height = h;
            self.bar.layer.commit();
            self.needs_redraw = true;
        }
    }

    fn open_menu(&mut self, id: menu::MenuId) {
        if self.menu_open == Some(id) {
            self.close_overlays();
            return;
        }
        if self.menu.is_none() {
            self.close_overlays();
            // Full screen, above everything, over the bar: a click anywhere
            // outside the menu closes it.
            self.menu = Some(make_layer_surface(
                &self.compositor,
                &self.layer_shell,
                &self.qh,
                Layer::Overlay,
                "solyra-menu",
                Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                -1,
                (0, 0, 0, 0),
                0,
                0,
                KeyboardInteractivity::Exclusive,
            ));
        }
        self.menu_open = Some(id);
        self.menu_armed = None;
        self.needs_redraw = true;
    }

    fn menu_titles(&self) -> Vec<menu::Title> {
        self.bar
            .regions
            .iter()
            .filter_map(|r| match r.action {
                HitAction::OpenMenu(id) => Some(menu::Title {
                    id,
                    x: r.x + 6,
                    w: r.w - 12,
                }),
                _ => None,
            })
            .collect()
    }

    fn menu_app_ctx(&self) -> (String, Option<String>, bool) {
        let (name, _) = state::focused_app_menus(&self.wins);
        let focused = self.wins.iter().find(|w| w.is_focused);
        let launch = focused.and_then(|w| {
            let role = state::role_for_app_id(&w.app_id);
            match role.as_str() {
                "terminal" => Some("solyra-open terminal".to_string()),
                "files" => Some("solyra-open files".to_string()),
                _ if !w.app_id.is_empty()
                    && w.app_id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) =>
                {
                    Some(w.app_id.clone())
                }
                _ => None,
            }
        });
        (name, launch, focused.is_some())
    }

    fn ensure_island(&mut self) {
        use solyra_conf::look::NotchStyle;
        let want = self.prefs.look.notch != NotchStyle::Off;
        let mat = self.prefs.look.material(self.prefs.look.notch_material);
        if mat != self.notch_material {
            // The namespace carries the material: make the surface anew.
            self.island = None;
            self.notch_material = mat;
        }
        match (want, self.island.is_some()) {
            (true, false) => {
                // Exclusive zone -1: sit at the very top, over the bar, ignoring
                // its reserved space.
                let slot = make_layer_surface(
                    &self.compositor,
                    &self.layer_shell,
                    &self.qh,
                    Layer::Overlay,
                    &material::namespace("solyra-notch", mat),
                    Anchor::TOP,
                    -1,
                    (0, 0, 0, 0),
                    island::SURF_W,
                    island::SURF_H,
                    KeyboardInteractivity::OnDemand,
                );
                self.island = Some(slot);
                self.island_anim = island::Anim::new(island::target(
                    self.prefs.look.notch,
                    &self.island_content(),
                    false,
                ));
                self.island_expanded = false;
            }
            (false, true) => {
                self.island = None;
                self.island_expanded = false;
            }
            _ => {}
        }
        self.retarget_island();
    }

    /// What the island shows, most important first: something waiting on
    /// the human, a fresh message, what is playing, an agent at work.
    fn island_content(&self) -> island::Content {
        if let Some((i, t)) = self
            .agent_tasks
            .iter()
            .enumerate()
            .find(|(_, t)| t.pending.is_some())
        {
            let p = t.pending.as_ref().expect("filtered");
            return island::Content::Attention(island::Ask {
                task: i,
                who: t.agent.clone().unwrap_or_else(|| t.name.clone()),
                kind: p.kind,
                text: p.action.clone(),
                resource: p.resource.clone(),
                reversible: p.reversible,
                task_scope: p.scopes.contains(&agent::Scope::Task),
                choices: p.choices.clone(),
                sent: t.decision_sent.is_some(),
            });
        }
        if let Some(msg) = &self.island_msg {
            return island::Content::Message(msg.clone());
        }
        if let Some(m) = self.media.lock().ok().and_then(|m| m.clone()) {
            if m.playing || !m.title.is_empty() {
                return island::Content::Media(m);
            }
        }
        let now = agent::now_secs();
        if let Some(t) = self
            .agent_tasks
            .iter()
            .find(|t| t.status == agent::Status::Running && !t.is_quiet(now))
        {
            return island::Content::Activity {
                who: t.agent.clone().unwrap_or_else(|| t.name.clone()),
                text: t
                    .latest_milestone()
                    .map(|m| m.text.clone())
                    .unwrap_or_else(|| "Working".into()),
            };
        }
        island::Content::Idle
    }

    /// Point the island's animation at where it should be now.
    fn retarget_island(&mut self) {
        if self.island.is_none() {
            return;
        }
        let to = island::target(
            self.prefs.look.notch,
            &self.island_content(),
            self.island_expanded,
        );
        if to != self.island_anim.to {
            self.island_anim.to = to;
            self.island_last_frame = Instant::now();
        }
        self.needs_redraw = true;
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

        self.update_dock_autohide();
        self.ensure_dock();
        self.ensure_desk();
        if let Some(t) = self.island_left_at {
            if self.island_expanded && t.elapsed() >= Duration::from_secs(4) && !self.reply_mode {
                self.island_expanded = false;
                self.island_left_at = None;
            }
        }
        self.retarget_island();
    }

    fn redraw(&mut self, conn: &Connection) {
        let scale = self.output_scale();
        if std::mem::take(&mut self.island_frame_only) {
            // An animation frame: only the island changes.
            self.draw_island(scale);
            let _ = conn.flush();
            return;
        }
        let telem = read_telemetry();
        let th = self.theme();
        let agents_summary = self.agent_summary;

        if self.bar.configured && self.bar.width > 0 {
            let hover = self.hover_on(&self.bar.surface);
            let w = self.bar.width;
            let h = self.bar.height;
            if let Ok((buffer, bytes)) = self.pool.create_buffer(
                (w * scale) as i32,
                (h * scale) as i32,
                (w * scale * 4) as i32,
                Format::Argb8888,
            ) {
                let mut cv = Canvas::new(bytes, w, h, scale as i32);
                self.bar.regions = if self.prefs.look.bar == solyra_conf::look::BarStyle::Hidden {
                    Vec::new()
                } else {
                    draw_bar(
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
                    )
                };
                let bar_shown = self.prefs.look.bar != solyra_conf::look::BarStyle::Hidden;
                // Only Acrylic blurs the bar: a blurred strip over a sharp
                // wallpaper would show as a band, and Glass keeps the sky clear.
                let rects = if bar_shown && self.bar_material == material::Material::Acrylic {
                    vec![(0, 0, w as i32, h as i32)]
                } else {
                    Vec::new()
                };
                set_blur(self.blur_mgr.as_ref(), &self.compositor, &self.qh, &mut self.bar, rects);
                present(
                    buffer,
                    &self.bar.surface,
                    w * scale,
                    h * scale,
                    scale as i32,
                );
            }
        }

        if self.dock.configured && self.dock.width > 0 {
            let raw_hover = self.hover_on(&self.dock.surface);
            // pearOS smooths the magnification pointer: it lerps toward the
            // real position instead of snapping, which is the fluid feel.
            let mut residual = 0.0f32;
            let hover = match raw_hover {
                Some((hx, hy)) => {
                    let cur = self.dock_mouse_smooth.unwrap_or(hx as f32);
                    let next = cur + (hx as f32 - cur) * 0.3;
                    self.dock_mouse_smooth = Some(next);
                    residual = (hx as f32 - next).abs();
                    Some((next as f64, hy))
                }
                None => {
                    self.dock_mouse_smooth = None;
                    None
                }
            };
            self.launching
                .retain(|(_, t0)| t0.elapsed() < Duration::from_millis(3000));
            let animating = !self.launching.is_empty() || residual > 0.5;
            let w = self.dock.width;
            let h = self.dock.height;
            if let Ok((buffer, bytes)) = self.pool.create_buffer(
                (w * scale) as i32,
                (h * scale) as i32,
                (w * scale * 4) as i32,
                Format::Argb8888,
            ) {
                let mut cv = Canvas::new(bytes, w, h, scale as i32);
                let live = Live {
                    agents: agents_summary,
                    telem: &telem,
                    prefs: &self.prefs,
                };
                let (regions, body) = dock::draw(
                    &mut cv,
                    &th,
                    &self.fonts,
                    self.prefs.dock_style,
                    &self.wins,
                    &mut self.icons,
                    hover,
                    &live,
                    self.prefs.look.dock_magnify,
                    self.dock_hidden && self.prefs.look.dock_autohide,
                    &self.launching,
                );
                self.dock.regions = regions;
                let rects = match body {
                    Some(b) if material::blurs(self.dock_material) => {
                        material::blur_rects(b.x, b.y, b.w, b.h, b.r)
                    }
                    _ => Vec::new(),
                };
                set_blur(self.blur_mgr.as_ref(), &self.compositor, &self.qh, &mut self.dock, rects);
                present(
                    buffer,
                    &self.dock.surface,
                    w * scale,
                    h * scale,
                    scale as i32,
                );
                if animating && !self.prefs.look.reduced_motion {
                    // Keep drawing frames while icons bounce or the smoothed
                    // pointer is still catching up.
                    self.dock
                        .surface
                        .frame(&self.qh, self.dock.surface.clone());
                }
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
                            (w * scale) as i32,
                            (h * scale) as i32,
                            (w * scale * 4) as i32,
                            Format::Argb8888,
                        ) {
                            let mut $cv = Canvas::new(bytes, w, h, scale as i32);
                            slot.regions = $draw;
                            present(buffer, &slot.surface, w * scale, h * scale, scale as i32);
                        }
                    }
                }
            };
        }

        self.sync_launcher();
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
        let notifs_media = self.media.lock().ok().and_then(|g| g.clone());
        overlay!(self.notifs_surface, |cv, hover| draw_notifs(
            &mut cv,
            &th,
            &self.fonts,
            &self.notifs,
            notifs_media.as_ref(),
            hover
        ));
        overlay!(self.dock_ctx, |cv, hover| menu::draw_ctx(
            &mut cv,
            &th,
            &self.fonts,
            &self.dock_ctx_entries,
            hover
        ));
        let telem = read_telemetry();
        let about_specs = self.about_specs(&telem);
        overlay!(self.about, |cv, _hover| draw_about(
            &mut cv,
            &th,
            &self.fonts,
            &about_specs
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
        let clock = chrono::Local::now()
            .format(self.prefs.look.clock_format())
            .to_string();
        let date = chrono::Local::now().format("%a %-d %b").to_string().to_uppercase();
        let project_lines = desk::project_lines();
        let project_refs: Vec<&str> = project_lines.iter().map(String::as_str).collect();
        let view = AgentsView {
            runtime_present: agents_summary.runtime_present,
            tasks: &self.agent_tasks,
            selected: self.agents_selected,
            now: agent::now_secs(),
            root_hint: &root_hint,
            error: self.agents_error.as_deref(),
            reply: &self.reply_buf,
            typing: self.reply_mode || self.typing_question(),
            desk: self.prefs.look.desktop == solyra_conf::look::DesktopMode::Agent,
            clock: &clock,
            date: &date,
            project_lines: &project_refs,
            environment: self.prefs.agent_env,
            agents_running: agents_summary
                .runtime_present
                .then_some(agents_summary.live),
        };
        overlay!(self.agents, |cv, hover| draw_agents(
            &mut cv,
            &th,
            &self.fonts,
            &view,
            hover
        ));

        if let (Some(id), Some(slot)) = (self.menu_open, self.menu.as_ref()) {
            let _ = slot;
            let titles = self.menu_titles();
            let (name, launch, has_window) = self.menu_app_ctx();
            let app = menu::AppCtx {
                name: &name,
                launch,
                has_window,
            };
            let entries = menu::entries(id, &app, self.prefs.theme == tokens::ThemeKind::Dark);
            let title = titles
                .iter()
                .copied()
                .find(|t| t.id == id)
                .unwrap_or(menu::Title { id, x: 12, w: 20 });
            let bar_h = self.bar.height as i32;
            let armed = self.menu_armed;
            overlay!(self.menu, |cv, hover| menu::draw(
                &mut cv,
                &th,
                &self.fonts,
                title,
                &titles,
                &entries,
                bar_h,
                armed,
                hover
            ));
        }

        self.draw_island(scale);
        self.draw_desk(scale, &th);
        let _ = conn.flush();
    }

    /// Agent workspace: a glass card on the right with clock, project, and chips.
    fn draw_desk(&mut self, scale: u32, th: &Theme) {
        let hover = self.desk.as_ref().and_then(|slot| {
            if self.pointer_surface.as_ref() == Some(&slot.surface) {
                Some(self.pointer_pos)
            } else {
                None
            }
        });
        let tint = self.prefs.wall_tint;
        let environment = self.prefs.agent_env;
        let running = self
            .agent_summary
            .runtime_present
            .then_some(self.agent_summary.live);
        let project = desk::project_lines();
        let project_refs: Vec<&str> = project.iter().map(String::as_str).collect();
        let Some(slot) = self.desk.as_mut() else {
            return;
        };
        if !slot.configured || slot.width == 0 {
            return;
        }
        let (w, h) = (slot.width, slot.height);
        let Ok((buffer, bytes)) = self.pool.create_buffer(
            (w * scale) as i32,
            (h * scale) as i32,
            (w * scale * 4) as i32,
            Format::Argb8888,
        ) else {
            return;
        };
        let mut cv = Canvas::new(bytes, w, h, scale as i32);
        let local = chrono::Local::now();
        let clock = local.format(self.prefs.look.clock_format()).to_string();
        let date = local.format("%A %-d %B").to_string();
        let (_, _, hits) = desk::paint(
            &mut cv,
            th,
            &self.fonts,
            &desk::Glance {
                clock: &clock,
                date: &date,
                project_lines: &project_refs,
                environment,
                agents_running: running,
            },
            tint,
            hover,
            0,
            0,
        );
        slot.regions = hits
            .into_iter()
            .map(|t| Region {
                x: t.x,
                y: t.y,
                w: t.w,
                h: t.h,
                action: HitAction::SetAgentEnv(t.environment),
            })
            .collect();
        let rects = if material::blurs(self.desk_material) {
            material::blur_rects(0, 0, w as i32, h as i32, desk::RADIUS)
        } else {
            Vec::new()
        };
        set_blur(
            self.blur_mgr.as_ref(),
            &self.compositor,
            &self.qh,
            slot,
            rects,
        );
        present(buffer, &slot.surface, w * scale, h * scale, scale as i32);
    }

    fn ensure_desk(&mut self) {
        use solyra_conf::look::DesktopMode;
        let want = self.prefs.look.desktop == DesktopMode::Agent;
        let mat = self.prefs.look.material(self.prefs.look.dock_material);
        if !want {
            if self.desk.is_some() {
                self.desk = None;
                self.needs_redraw = true;
            }
            return;
        }
        if self.desk.is_some() && mat == self.desk_material {
            return;
        }
        self.desk = Some(make_layer_surface(
            &self.compositor,
            &self.layer_shell,
            &self.qh,
            Layer::Bottom,
            &material::namespace("solyra-desk", mat),
            Anchor::TOP | Anchor::RIGHT,
            0,
            (56, 18, 0, 0),
            desk::W as u32,
            desk::H as u32,
            KeyboardInteractivity::None,
        ));
        self.desk_material = mat;
        self.needs_redraw = true;
    }

    fn draw_island(&mut self, scale: u32) {
        let content = self.island_content();
        let now = Instant::now();
        let dt = now
            .duration_since(self.island_last_frame)
            .as_secs_f32()
            .min(0.05);
        self.island_last_frame = now;
        let moving = self.island_anim.step(dt, self.prefs.look.reduced_motion);
        let Some(slot) = self.island.as_mut() else {
            return;
        };
        if !slot.configured || slot.width == 0 {
            return;
        }
        let hover = self
            .pointer_surface
            .as_ref()
            .filter(|s| *s == &slot.surface)
            .map(|_| self.pointer_pos);
        let (w, h) = (slot.width, slot.height);
        let local = chrono::Local::now();
        let clock = local.format(self.prefs.look.clock_format()).to_string();
        let date = local.format("%A %-d %B").to_string();
        let telem = read_telemetry();
        let glance = island::Glance {
            battery: telem.battery_pct,
            network: telem.network.label().to_string(),
            agents_running: self.agent_summary.live,
            windows: self.wins.len(),
        };
        let typing = matches!(&content, island::Content::Attention(a) if a.kind == agent::AskKind::Question && a.choices.is_empty() && !a.sent);
        let extra = island::Extra {
            glance: &glance,
            clock: &clock,
            date: &date,
            reply: &self.reply_buf,
            typing,
        };
        let Ok((buffer, bytes)) = self.pool.create_buffer(
            (w * scale) as i32,
            (h * scale) as i32,
            (w * scale * 4) as i32,
            Format::Argb8888,
        ) else {
            return;
        };
        let mut cv = Canvas::new(bytes, w, h, scale as i32);
        slot.regions = island::draw(
            &mut cv,
            &self.fonts,
            self.prefs.look.notch,
            &self.island_anim,
            &content,
            self.island_expanded,
            &extra,
            hover,
        );
        // Only the shape takes the pointer; the rest of the surface is clear
        // and clicks there reach whatever is below.
        let (ix, iy, iw, ih) = island::input_rect(&self.island_anim.cur);
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(&self.compositor) {
            region.add(ix - 10, iy, iw + 20, ih);
            slot.surface.set_input_region(Some(region.wl_region()));
        }
        // The island is opaque black: there is nothing to blur behind it.
        set_blur(self.blur_mgr.as_ref(), &self.compositor, &self.qh, slot, Vec::new());
        if moving {
            // Ask for the next frame; `CompositorHandler::frame` redraws.
            slot.surface.frame(&self.qh, slot.surface.clone());
        }
        present(buffer, &slot.surface, w * scale, h * scale, scale as i32);
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
            &self.notifs_surface,
            &self.dock_ctx,
            &self.about,
            &self.island_card,
            &self.agents,
            &self.desk,
        ]
        .into_iter()
        .flatten()
        .find(|s| &s.surface == surface)
        .map(|s| &s.regions)
    }

    fn handle_click(&mut self, surface: &WlSurface, x: f64, y: f64) {
        let on_island = self.island.as_ref().is_some_and(|s| &s.surface == surface);
        let on_menu = self.menu.as_ref().is_some_and(|s| &s.surface == surface);
        let hit = if on_island {
            // Island regions overlap (the whole shape, then its buttons):
            // the most specific wins.
            self.island
                .as_ref()
                .and_then(|s| island::pick(&s.regions, x, y))
        } else {
            self.regions_for(surface).and_then(|regions| {
                regions
                    .iter()
                    .find(|r| r.contains(x, y))
                    .map(|r| r.action.clone())
            })
        };
        let Some(hit) = hit else {
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
            HitAction::CloseWindow(id) => {
                send_ipc(&format!(
                    "{{\"Action\":{{\"CloseWindow\":{{\"id\":{id}}}}}}}"
                ));
                self.close_overlays();
            }
            HitAction::Launch(target) => {
                if surface == &self.dock.surface {
                    self.launching
                        .push((dock::launch_key(&target), Instant::now()));
                    self.needs_redraw = true;
                }
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
            HitAction::Choose { task, choice } if on_island => {
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
            HitAction::OpenMenu(id) => {
                self.open_menu(id);
                return;
            }
            HitAction::ArmMenuItem(i) => {
                self.menu_armed = Some(i);
                self.needs_redraw = true;
                return;
            }
            HitAction::Media(control) => {
                if let Some(m) = self.media.lock().ok().and_then(|m| m.clone()) {
                    media::control(m.bus, control);
                }
            }
            HitAction::StartReply if on_island => {
                // Typing in the island goes to the task it is showing.
                if let island::Content::Attention(a) = self.island_content() {
                    self.agents_selected = a.task;
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
            HitAction::SetAgentEnv(which) => {
                if which < desk::ENVIRONMENTS.len() as u8 && which != self.prefs.agent_env {
                    self.prefs.agent_env = which;
                    self.save_prefs();
                }
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
        if on_menu && self.menu.is_some() {
            // A menu item ran: the menu closes, as menus do.
            self.close_overlays();
        }
        self.needs_state_sync = true;
        self.needs_redraw = true;
    }

    /// Spec lines for the "About This Computer" card.
    fn about_specs(&self, telem: &state::Telemetry) -> Vec<(String, String)> {
        let os = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("PRETTY_NAME="))
                    .map(|l| {
                        l.trim_start_matches("PRETTY_NAME=")
                            .trim_matches('"')
                            .to_string()
                    })
            })
            .unwrap_or_else(|| "Solyra Linux".into());
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let chip = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split(':').nth(1).map(|v| v.trim().to_string()))
            })
            .unwrap_or_else(|| "Unknown processor".into());
        let memory = format!("{:.0} GB", telem.mem_total_mib as f64 / 1024.0);
        let compositor = state::compositor_version()
            .map(|v| format!("niri {v}"))
            .unwrap_or_else(|| "niri".into());
        vec![
            ("OS".into(), os),
            ("Kernel".into(), kernel),
            ("Chip".into(), chip),
            ("Memory".into(), memory),
            ("Compositor".into(), compositor),
        ]
    }

    /// Right-click: a dock icon's context card (macOS Dock style).
    fn handle_right_click(&mut self, surface: &WlSurface, x: f64, y: f64) {
        if &self.dock.surface != surface {
            return;
        }
        let Some(region) = self.dock.regions.iter().find(|r| r.contains(x, y)) else {
            return;
        };
        let icon_cx = region.x + region.w / 2;
        let mut entries = Vec::new();
        match region.action.clone() {
            HitAction::FocusWindow(id) => {
                entries.push(menu::ctx_item("Open".into(), HitAction::FocusWindow(id)));
                entries.push(menu::ctx_sep());
                entries.push(menu::ctx_item("Quit".into(), HitAction::CloseWindow(id)));
            }
            HitAction::Launch(t) => {
                let is_trash = matches!(&t, LaunchTarget::Shell(c) if c.contains("trash:///"));
                entries.push(menu::ctx_item("Open".into(), HitAction::Launch(t)));
                if is_trash {
                    entries.push(menu::ctx_sep());
                    entries.push(menu::ctx_item(
                        "Empty Trash".into(),
                        HitAction::Launch(LaunchTarget::Shell(
                            "rm -rf ~/.local/share/Trash/files/* ~/.local/share/Trash/info/*"
                                .into(),
                        )),
                    ));
                }
            }
            _ => return,
        }
        let (mw, mh) = menu::panel_size(&self.fonts, &entries);
        // The dock is centred on the output: recover the icon's global x.
        let out_w = self
            .output_state
            .outputs()
            .filter_map(|o| self.output_state.info(&o))
            .filter_map(|i| i.logical_size.map(|(w, _)| w))
            .max()
            .unwrap_or(0);
        let spec = dock::spec(
            self.prefs.dock_style,
            &self.wins,
            self.prefs.look.dock_magnify,
            self.dock_hidden,
        );
        let global_cx = ((out_w - spec.width as i32) / 2).max(0) + icon_cx;
        let left = (global_cx - mw / 2).clamp(8, (out_w - mw - 8).max(8));
        let bottom = spec.bottom_margin + spec.height as i32 + 6;
        let slot = self.open_overlay(
            "solyra-dock-ctx",
            Anchor::BOTTOM | Anchor::LEFT,
            (0, 0, bottom, left),
            (mw as u32, mh as u32),
            KeyboardInteractivity::OnDemand,
        );
        self.dock_ctx = Some(slot);
        self.dock_ctx_entries = entries;
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
        if self.island_expanded && self.launcher.is_none() {
            if let island::Content::Attention(a) = self.island_content() {
                // The island shows one task: keys answer it (A/D/T, 1-9, or
                // typing for a question).
                self.agents_selected = a.task;
                self.handle_agents_key(key);
                self.retarget_island();
            }
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
                    .filter(|_| !self.launcher_query.trim().is_empty())
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

/// Attach `buffer` (already `scale` times the logical size) and tell the
/// compositor so, so it maps one buffer pixel to one device pixel.
/// Start the lock screen in its own process (a crash there must never take
/// the shell down, and the compositor keeps the session locked if it dies).
/// Falls back to swaylock if Solyra's lock screen cannot start.
fn spawn_lock() {
    let exe = std::env::current_exe().unwrap_or_else(|_| "solyra-panel".into());
    let ok = std::process::Command::new(&exe)
        .arg("lock")
        .stdin(std::process::Stdio::null())
        .spawn()
        .is_ok();
    if !ok {
        let _ = std::process::Command::new("swaylock").spawn();
    }
}

fn present(
    buffer: smithay_client_toolkit::shm::slot::Buffer,
    surface: &WlSurface,
    width: u32,
    height: u32,
    scale: i32,
) {
    surface.set_buffer_scale(scale);
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
        self.needs_redraw = true;
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &WlSurface, _: u32) {
        if self.island.as_ref().is_some_and(|s| &s.surface == surface) {
            self.needs_redraw = true;
            self.island_frame_only = true;
        }
        if surface == &self.dock.surface {
            self.needs_redraw = true;
        }
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
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {
        self.needs_redraw = true;
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {
        // A display's scale or size changed: redraw at the new scale.
        self.needs_redraw = true;
    }
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
            &mut self.notifs_surface,
            &mut self.dock_ctx,
            &mut self.about,
            &mut self.island_card,
            &mut self.agents,
            &mut self.island,
            &mut self.menu,
            &mut self.desk,
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
            &mut self.notifs_surface,
            &mut self.dock_ctx,
            &mut self.about,
            &mut self.island_card,
            &mut self.agents,
            &mut self.island,
            &mut self.menu,
            &mut self.desk,
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
                    if self
                        .island
                        .as_ref()
                        .is_some_and(|s| s.surface == event.surface)
                    {
                        self.island_left_at = None;
                    }
                    if event.surface == self.dock.surface {
                        self.dock_left_at = None;
                        if self.dock_hidden {
                            // Touching the edge strip brings an auto-hidden dock back.
                            self.dock_hidden = false;
                            self.ensure_dock();
                        }
                    }
                    self.needs_redraw = true;
                }
                PointerEventKind::Leave { .. } => {
                    if self.island_expanded
                        && self
                            .island
                            .as_ref()
                            .is_some_and(|s| s.surface == event.surface)
                    {
                        self.island_left_at = Some(Instant::now());
                        let wake = self.waker.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(4100));
                            wake();
                        });
                    }
                    if event.surface == self.dock.surface && self.prefs.look.dock_autohide {
                        self.dock_left_at = Some(Instant::now());
                        let wake = self.waker.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(750));
                            wake();
                        });
                    }
                    self.pointer_surface = None;
                    self.needs_redraw = true;
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_pos = event.position;
                    if self
                        .menu
                        .as_ref()
                        .is_some_and(|s| s.surface == event.surface)
                    {
                        let (px, py) = event.position;
                        let over = self.menu.as_ref().and_then(|s| {
                            s.regions
                                .iter()
                                .find(|r| r.contains(px, py))
                                .map(|r| r.action.clone())
                        });
                        if let Some(HitAction::OpenMenu(id)) = over {
                            if self.menu_open != Some(id) {
                                self.menu_open = Some(id);
                                self.menu_armed = None;
                            }
                        }
                    }
                    self.needs_redraw = true;
                }
                PointerEventKind::Press { button, .. } if *button == 0x110 => {
                    let surface = event.surface.clone();
                    let (x, y) = event.position;
                    self.handle_click(&surface, x, y);
                }
                PointerEventKind::Press { button, .. } if *button == 0x111 => {
                    let surface = event.surface.clone();
                    let (x, y) = event.position;
                    self.handle_right_click(&surface, x, y);
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

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        _: ext_background_effect_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // `capabilities`: blur is the only one we use; if it is missing the
        // compositor simply ignores our regions.
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectSurfaceV1,
        _: ext_background_effect_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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

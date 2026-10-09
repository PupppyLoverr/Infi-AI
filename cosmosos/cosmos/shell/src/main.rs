//! cosmos-shell — the Cosmos desktop shell: top panel, app launcher,
//! notification popups. Runs as a wlr-layer-shell Wayland client and talks
//! to cosmos-compositor over cosmos-ipc.

mod activity;
mod ask;
mod assist;
mod ccdetail;
mod clipwatch;
mod desktop;
mod dock;
mod draw;
mod fileindex;
mod glass;
mod help;
mod icons;
mod ipc_client;
mod island;
mod launcher;
mod logind;
mod menubar;
mod notify;
mod panel;
mod popups;
mod preview;
mod quick;
mod search;
mod startw;
mod switcher;
mod sysinfo;
mod weather;
mod widgets;
mod zoomflyout;

const DEFAULT_WALLPAPER: &str = "violet";

thread_local! {
    /// Mirror of ShellState::lite — glass draws flat while set.
    pub static LITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Mirror of ShellState::wallpaper so static helpers can reach it.
    /// Starts at the same default: apply_config only writes it on a change,
    /// so an empty start left glass with no wallpaper on a fresh account.
    static WALLPAPER: std::cell::RefCell<String> =
        std::cell::RefCell::new(DEFAULT_WALLPAPER.to_string());
    /// Mirror of ShellState::dark — picks the `-light` wallpaper variant.
    static DARK: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

use std::{os::unix::net::UnixStream, time::Duration};

use calloop::{
    channel::{self, Event as ChannelEvent},
    generic::Generic,
    timer::Timer,
    EventLoop, Interest, LoopHandle, Mode, PostAction,
};
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        DataDeviceManagerState,
    },
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, Proxy, QueueHandle,
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use desktop::AppEntry;
use ipc_client::IpcClient;
use notify::{Notification, NotifyEvent};
use sysinfo::SysInfo;

pub const PANEL_HEIGHT: u32 = 30;
pub const NOTIFY_WIDTH: u32 = 340;
pub const NOTIFY_TIMEOUT_MS: i64 = 5000;
pub const LAUNCHER_WIDTH: u32 = 760;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    logind::spawn_listener();

    if let Err(err) = run() {
        tracing::error!("cosmos-shell fatal: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let mut event_loop: EventLoop<ShellState> = EventLoop::try_new()?;
    let handle = event_loop.handle();

    let compositor_state = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);
    let pool = SlotPool::new(64 * 1024, &shm).map_err(|e| format!("slot pool: {e}"))?;

    let mut apps = desktop::scan_apps();
    apps.extend(desktop::system_entries());

    // Clipboard channel — `clipwatch` readers push copied text here;
    // drop channel delivers decoded uri-lists onto `staged_files`.
    // Index channel — the file-index thread ships its scan back here.
    let (clip_tx, clip_rx) = channel::channel::<String>();
    let (drop_tx, drop_rx) = channel::channel::<Vec<String>>();
    let (index_tx, index_rx) = channel::channel::<Vec<(String, String)>>();

    let mut state = ShellState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        layer_shell,
        shm,
        seat_state,
        output_state,
        pool,
        viewporter: globals
            .bind::<WpViewporter, _, _>(&qh, 1..=1, ScaleData)
            .ok(),
        viewports: Vec::new(),
        qh: qh.clone(),
        loop_handle: handle.clone(),
        panel: None,
        dock_surface: None,
        widgets_surface: None,
        widgets_size: (0, 0),
        widgets_dirty: false,
        show_widgets: true,
        weather: None,
        weather_city: String::new(),
        weather_tx: None,
        widgets_clock: String::new(),
        switcher_surface: None,
        launcher_surface: None,
        notify_surface: None,
        quick_surface: None,
        assist_surface: None,
        notify_conn: None,
        panel_size: (0, PANEL_HEIGHT),
        dock_size: (0, 0),
        dock_position: dock::DockPos::Bottom,
        switcher_size: (0, 0),
        switcher_order: Vec::new(),
        switcher_sel: 0,
        launcher_size: (0, 0),
        assist_size: (0, 0),
        assist_ids: Vec::new(),
        assist_fill_left: false,
        assist_sel: 0,
        assist_hover: None,
        assist_open: false,
        assist_dirty: false,
        zoom_surface: None,
        zoom_window: 0,
        zoom_hover: None,
        zoom_dirty: false,
        menu_surface: None,
        menu_open: None,
        pointer_on: None,
        menu_hover: None,
        menu_dirty: false,
        menu_pos: (0, 0),
        launcher_hover: None,
        notify_size: (0, 0),
        quick_size: (0, 0),
        windows: Vec::new(),
        workspaces: Vec::new(),
        launcher_open: false,
        launcher_query: String::new(),
        launcher_preset: None,
        launcher_search: false,
        launcher_preview: None,
        launcher_card_h: 0.0,
        start_todos: None,
        start_todo_edit: None,
        start_photo: None,
        ask_tx: None,
        ask_id: 0,
        ask_pid: None,
        ask_query: String::new(),
        ask_text: String::new(),
        ask_done: None,
        launcher_sel: 0,
        recent: launcher::load_recent(),
        apps,
        sysinfo: SysInfo::default(),
        notifications: Vec::new(),
        ipc: IpcClient::default(),
        ipc_retry_in: 5,
        dark: true,
        panel_dirty: true,
        launcher_dirty: false,
        notify_dirty: false,
        quick_dirty: false,
        panel_hover: (0.0, false),
        dock_hover: None,
        dock_dirty: true,
        switcher_dirty: false,
        quick_open: false,
        quick_dismissed_at: None,
        vol_drag: false,
        bright_drag: false,
        cc_detail: None,
        accent_name: cosmos_ipc::DEFAULT_ACCENT.to_string(),
        wallpaper: DEFAULT_WALLPAPER.to_string(),
        help_surface: None,
        help_size: (0, 0),
        help_open: false,
        help_dirty: false,
        island_surface: None,
        island_open: false,
        island_hover: None,
        island_tab: island::Tab::Activities,
        island_anim: None,
        island_anim_timer: false,
        island_dirty: false,
        island_dismissed_at: None,
        clip_history: std::collections::VecDeque::new(),
        staged_files: Vec::new(),
        clip_manager: None,
        clip_device: None,
        clip_offers: std::collections::HashMap::new(),
        clip_tx,
        clip_pending_text: String::new(),
        clip_source: None,
        data_device_manager: None,
        data_devices: Vec::new(),
        drop_tx,
        dnd_on_island: false,
        agents: activity::scan_agents(),
        media: None,
        lite: false,
        dnd: false,
        file_index: Vec::new(),
        index_tx,
        index_refreshed: std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(120))
            .unwrap_or_else(std::time::Instant::now),
        exit: false,
    };

    state.data_device_manager = Some(
        DataDeviceManagerState::bind(&globals, &qh).map_err(|e| format!("data device mgr: {e}"))?,
    );

    state.create_panel(&qh);
    state.create_dock(&qh);
    state.create_widgets(&qh);
    let (weather_tx, weather_rx) = channel::channel::<Option<weather::Weather>>();
    state.weather_tx = Some(weather::start(weather_tx, String::new()));
    let (ask_tx, ask_rx) = channel::channel::<(u64, ask::Event)>();
    state.ask_tx = Some(ask_tx);
    handle.insert_source(ask_rx, |event, _, state| {
        if let ChannelEvent::Msg((id, ev)) = event {
            if id != state.ask_id {
                return;
            }
            match ev {
                ask::Event::Started(pid) => state.ask_pid = Some(pid),
                ask::Event::Chunk(s) => state.ask_text.push_str(&s),
                ask::Event::Done(r) => {
                    state.ask_pid = None;
                    state.ask_done = Some(r);
                }
            }
            state.launcher_dirty = true;
        }
    })?;
    handle.insert_source(weather_rx, |event, _, state| {
        if let ChannelEvent::Msg(w) = event {
            if w != state.weather {
                let resize = w.is_some() != state.weather.is_some();
                state.weather = w;
                if resize {
                    state.resize_widgets();
                }
                state.widgets_dirty = true;
            }
        }
    })?;
    clipwatch::init(&mut state, &globals);
    if let Some(reader) = state.ipc.connect() {
        register_ipc_source(&handle, reader);
    } else {
        tracing::warn!("compositor IPC unavailable at startup; retrying on a timer");
    }

    // System probes (clock/battery/network/audio) on a worker thread.
    let (sys_tx, sys_rx) = channel::channel::<SysInfo>();
    sysinfo::start(sys_tx);
    handle.insert_source(sys_rx, |event, _, state| {
        if let ChannelEvent::Msg(info) = event {
            state.sysinfo = info;
            state.panel_dirty = true;
            state.quick_dirty = state.quick_open;
            let minute = widgets::clock_lines().0;
            if minute != state.widgets_clock {
                state.widgets_clock = minute;
                state.widgets_dirty = true;
            }
        }
    })?;

    handle.insert_source(clip_rx, |event, _, state| {
        if let ChannelEvent::Msg(text) = event {
            clipwatch::note_clip(state, text);
        }
    })?;
    handle.insert_source(index_rx, |event, _, state| {
        if let ChannelEvent::Msg(entries) = event {
            state.file_index = entries;
        }
    })?;
    handle.insert_source(drop_rx, |event, _, state| {
        if let ChannelEvent::Msg(paths) = event {
            for path in paths {
                if !state.staged_files.contains(&path) {
                    state.staged_files.push(path);
                }
            }
            state.island_dirty = true;
            state.panel_dirty = true;
        }
    })?;

    // Freedesktop notification service.
    let (notify_tx, notify_rx) = channel::channel::<NotifyEvent>();
    match notify::start(notify_tx) {
        Ok(conn) => {
            state.notify_conn = Some(conn);
            tracing::info!("notifications service registered");
        }
        Err(err) => tracing::warn!("notification service failed: {err}"),
    }
    handle.insert_source(notify_rx, |event, _, state| {
        if let ChannelEvent::Msg(ev) = event {
            state.on_notify(ev);
        }
    })?;

    // MPRIS now-playing for the island.
    let (media_tx, media_rx) = channel::channel::<Option<activity::Media>>();
    activity::start_mpris(media_tx);
    handle.insert_source(media_rx, |event, _, state| {
        if let ChannelEvent::Msg(m) = event {
            state.media = m;
            state.panel_dirty = true;
            state.refresh_island();
        }
    })?;

    // Island capsule pulse: ~12 fps only while an approval waits or an
    // agent works (never in Lite Mode); otherwise a cheap 1s re-check.
    handle.insert_source(
        Timer::from_duration(Duration::from_millis(80)),
        |_, _, state| {
            if island::pulsing(state) {
                state.panel_dirty = true;
                calloop::timer::TimeoutAction::ToDuration(Duration::from_millis(80))
            } else {
                calloop::timer::TimeoutAction::ToDuration(Duration::from_secs(1))
            }
        },
    )?;

    // 1s tick: repaint clock + expire notifications + IPC reconnect.
    handle.insert_source(
        Timer::from_duration(Duration::from_secs(1)),
        |_, _, state| {
            state.on_tick();
            calloop::timer::TimeoutAction::ToDuration(Duration::from_secs(1))
        },
    )?;

    WaylandSource::new(conn, queue).insert(handle)?;

    while !state.exit {
        event_loop.dispatch(None, &mut state)?;
        state.render();
    }
    Ok(())
}

fn register_ipc_source(handle: &LoopHandle<'static, ShellState>, stream: UnixStream) {
    let _ = handle.insert_source(
        Generic::new(stream, Interest::READ, Mode::Level),
        |_, _, state| {
            if !state.ipc.read() {
                tracing::warn!("compositor IPC closed; will retry");
                return Ok(PostAction::Remove);
            }
            for ev in state.ipc.poll_events() {
                state.on_ipc_event(ev);
            }
            Ok(PostAction::Continue)
        },
    );
}

pub struct ShellState {
    pub registry_state: RegistryState,
    pub compositor_state: CompositorState,
    pub layer_shell: LayerShell,
    pub shm: Shm,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub pool: SlotPool,
    /// wp_viewporter, for painting at the fractional output scale.
    pub viewporter: Option<WpViewporter>,
    /// Viewport per layer surface, created on first paint.
    pub viewports: Vec<(wl_surface::WlSurface, WpViewport)>,
    pub qh: QueueHandle<Self>,
    pub loop_handle: LoopHandle<'static, Self>,

    pub panel: Option<LayerSurface>,
    pub dock_surface: Option<LayerSurface>,
    /// Desktop clock/weather cards (layer Bottom), when enabled.
    pub widgets_surface: Option<LayerSurface>,
    pub widgets_size: (u32, u32),
    pub widgets_dirty: bool,
    /// Config `desktop_widgets`.
    pub show_widgets: bool,
    /// Latest Open-Meteo reading; `None` (offline/unknown place) hides the card.
    pub weather: Option<weather::Weather>,
    /// Config `weather_city` as last sent to the weather thread.
    pub weather_city: String,
    pub weather_tx: Option<std::sync::mpsc::Sender<String>>,
    /// Minute shown on the clock card (redraw only when it changes).
    pub widgets_clock: String,
    pub switcher_surface: Option<LayerSurface>,
    pub launcher_surface: Option<LayerSurface>,
    pub notify_surface: Option<LayerSurface>,
    pub quick_surface: Option<LayerSurface>,
    /// Snap Assist picker (Win11) — fullscreen overlay on the free half.
    pub assist_surface: Option<LayerSurface>,
    /// Zoom flyout (Win11 snap layouts) — small card under the green
    /// zoom button.
    pub zoom_surface: Option<LayerSurface>,
    pub notify_conn: Option<zbus::blocking::Connection>,
    pub panel_size: (u32, u32),
    pub dock_size: (u32, u32),
    pub switcher_size: (u32, u32),
    /// Window ids in compositor cycle order while the switcher is open.
    pub switcher_order: Vec<u64>,
    pub switcher_sel: u64,
    pub launcher_size: (u32, u32),
    /// Snap Assist state — the free-half picker after a Left/Right snap.
    pub assist_size: (u32, u32),
    /// Candidate window ids the picker offers (compositor order).
    pub assist_ids: Vec<u64>,
    /// True when the picker occupies the LEFT half (snap went right).
    pub assist_fill_left: bool,
    pub assist_sel: usize,
    pub assist_hover: Option<usize>,
    pub assist_open: bool,
    pub assist_dirty: bool,
    /// Zoom flyout state — the window id it's open for, hovered cell.
    pub zoom_window: u64,
    pub zoom_hover: Option<(usize, usize)>,
    pub zoom_dirty: bool,
    /// Menubar dropdown: surface, open menu index, hovered row, screen pos.
    pub menu_surface: Option<LayerSurface>,
    pub menu_open: Option<usize>,
    /// Surface under the pointer (wl_pointer enter/leave). The compositor
    /// moves keyboard focus before it delivers the press, so a click
    /// inside a menu can arrive after that menu's keyboard `leave`.
    pub pointer_on: Option<wl_surface::WlSurface>,
    pub menu_hover: Option<usize>,
    pub menu_dirty: bool,
    pub menu_pos: (i32, i32),
    /// Dynamic island card (clipboard history + staged files) — a Top
    /// layer surface centred under the menubar pill.
    pub island_surface: Option<LayerSurface>,
    pub island_open: bool,
    pub island_hover: Option<island::Hit>,
    pub island_tab: island::Tab,
    /// Running capsule↔card morph: (start, opening). A closing card keeps
    /// its surface until the reverse morph ends.
    pub island_anim: Option<(std::time::Instant, bool)>,
    pub island_anim_timer: bool,
    pub island_dirty: bool,
    /// Set when the island card just closed from a focus-loss `leave` —
    /// the pill click inside the window is the same physical click and
    /// must not reopen it (same guard as `quick_dismissed_at`).
    pub island_dismissed_at: Option<std::time::Instant>,
    /// Clipboard history ring (newest first) — fed by `clipwatch`.
    pub clip_history: std::collections::VecDeque<String>,
    /// Files dropped onto the island card, staged for later use.
    pub staged_files: Vec<String>,
    /// wlr-data-control clipboard watcher objects.
    pub clip_manager: Option<
        wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    >,
    pub clip_device: Option<
        wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::ZwlrDataControlDeviceV1,
    >,
    /// Mime types announced per offer object (keyed by object id).
    pub clip_offers:
        std::collections::HashMap<wayland_client::backend::ObjectId, Vec<String>>,
    /// Channel `clipwatch` readers push decoded clipboard text into.
    pub clip_tx: calloop::channel::Sender<String>,
    /// Text staged for `clipwatch::set_clipboard` sources.
    pub clip_pending_text: String,
    pub clip_source: Option<
        wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_source_v1::ZwlrDataControlSourceV1,
    >,
    /// Regular data-device objects — accepting file drops onto the
    /// island card (uri-list DnD → staged files).
    pub data_device_manager: Option<DataDeviceManagerState>,
    pub data_devices: Vec<DataDevice>,
    /// Channel decoded uri-list drops arrive on.
    pub drop_tx: calloop::channel::Sender<Vec<String>>,
    /// True while a drag hovers the island card.
    pub dnd_on_island: bool,
    /// Connected agents (cosmos-agentd live files) — island live activity.
    pub agents: Vec<activity::AgentActivity>,
    /// MPRIS now-playing player, if any.
    pub media: Option<activity::Media>,
    /// Lite Mode (config `lite_mode`): flat surfaces, see [`LITE`].
    pub lite: bool,
    /// Focus mode (Do Not Disturb) — notification popups are suppressed
    /// while on; history still records. Toggled in Control Centre.
    pub dnd: bool,
    /// Pointer hover inside the launcher card (cells, rows, footer).
    pub launcher_hover: Option<launcher::Hit>,
    /// Background file index feeding Search-mode file rows.
    pub file_index: Vec<(String, String)>,
    /// Completed index scans arrive here from the indexer thread.
    pub index_tx: calloop::channel::Sender<Vec<(String, String)>>,
    /// When the index was last (re)started.
    pub index_refreshed: std::time::Instant,
    pub notify_size: (u32, u32),
    pub quick_size: (u32, u32),

    pub windows: Vec<cosmos_ipc::WindowInfo>,
    pub workspaces: Vec<cosmos_ipc::WorkspaceInfo>,
    pub launcher_open: bool,
    pub launcher_query: String,
    /// Query the next launcher open starts with ("?" = Ask mode).
    pub launcher_preset: Option<String>,
    /// Open as Search or Ask (super+Space / Ask Cosmos) instead of Start.
    pub launcher_search: bool,
    /// Preview of the selected Files result (Search or Ask).
    pub launcher_preview: Option<preview::FilePreview>,
    /// Painted Search or Ask card height (pill + results/answer).
    pub launcher_card_h: f64,
    pub start_todos: Option<Vec<startw::Todo>>,
    pub start_todo_edit: Option<String>,
    pub start_photo: Option<(std::path::PathBuf, tiny_skia::Pixmap)>,
    pub ask_tx: Option<calloop::channel::Sender<(u64, ask::Event)>>,
    pub ask_id: u64,
    pub ask_pid: Option<u32>,
    /// Question being answered ("" = none).
    pub ask_query: String,
    pub ask_text: String,
    /// None while streaming; the outcome once opencode exits.
    pub ask_done: Option<Result<(), String>>,
    pub launcher_sel: usize,
    /// App launch MRU (desktop ids, newest first) — the launcher's
    /// RECOMMENDED section; persisted to ~/.local/share/cosmos-shell.
    pub recent: Vec<String>,
    pub apps: Vec<AppEntry>,
    pub sysinfo: SysInfo,
    pub notifications: Vec<Notification>,
    pub ipc: IpcClient,
    pub ipc_retry_in: u32,
    pub dark: bool,
    pub panel_dirty: bool,
    pub launcher_dirty: bool,
    pub notify_dirty: bool,
    pub quick_dirty: bool,
    /// (x, hovering) — last pointer x on the panel, for hit highlights.
    pub panel_hover: (f64, bool),
    /// Last pointer position along the dock rail's axis (cell
    /// highlight + magnification centre).
    pub dock_hover: Option<f64>,
    /// Which screen edge the dock rail sits on (config `dock_position`).
    pub dock_position: dock::DockPos,
    pub dock_dirty: bool,
    pub switcher_dirty: bool,
    /// Quick-settings flyout state.
    pub quick_open: bool,
    /// Set when the flyout just closed from a focus-loss `leave` — the
    /// next tray click inside the window is the same physical click and
    /// must not reopen it.
    pub quick_dismissed_at: Option<std::time::Instant>,
    /// Held while the pointer is dragging the volume slider.
    pub vol_drag: bool,
    /// Control Centre brightness slider held.
    pub bright_drag: bool,
    /// Control Centre detail view (Wi-Fi list / sound outputs).
    pub cc_detail: Option<ccdetail::View>,
    /// Current accent preset name (from Config events) — the Quick
    /// Settings swatch strip draws its selection ring from this.
    pub accent_name: String,
    /// Active wallpaper name — drives glass sampling; from config.
    pub wallpaper: String,
    /// Keybind cheatsheet (super+?) — fullscreen scrim + card.
    pub help_surface: Option<LayerSurface>,
    pub help_size: (u32, u32),
    pub help_open: bool,
    pub help_dirty: bool,
    pub exit: bool,
}

/// How many workspace cells the pager draws.
pub fn workspace_count(state: &ShellState) -> usize {
    state.workspaces.len().max(1)
}

// Fields needing `Option`/defaults for the manual Default impl.

impl ShellState {
    /// Wallpaper name for glass sampling (stringly-cloned at call
    /// sites that can't hold a borrow across the draw).
    pub fn wallpaper_name() -> String {
        // Sourced from the thread-local mirror kept in draw:: — updated
        // in apply_config alongside everything else.
        let name = WALLPAPER.with(|w| w.borrow().clone());
        cosmos_ipc::wallpaper_for(&name, DARK.with(|d| d.get()))
    }

    pub fn create_panel(&mut self, qh: &QueueHandle<Self>) {
        let surface = self.compositor_state.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("cosmos-panel"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, PANEL_HEIGHT);
        layer.set_exclusive_zone(PANEL_HEIGHT as i32);
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer.wl_surface().commit();
        self.panel = Some(layer);
        self.panel_dirty = true;
    }

    /// The dock — an edge rail (left by default, right/bottom via the
    /// `dock_position` config key). It reserves STRIP px of exclusive
    /// zone on its edge so maximised and snapped windows never slide
    /// underneath; the surface is wider than the zone by the label
    /// overhang, which stays transparent and input-free.
    /// Clock + weather cards, top-left under the menubar, below windows.
    pub fn create_widgets(&mut self, qh: &QueueHandle<Self>) {
        if !self.show_widgets || self.widgets_surface.is_some() {
            return;
        }
        let surface = self.compositor_state.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Bottom,
            Some("cosmos-widgets"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin(widgets::MARGIN, 0, 0, widgets::MARGIN);
        layer.set_size(widgets::W, widgets::height(self.weather.is_some()));
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.wl_surface().commit();
        self.widgets_surface = Some(layer);
    }

    fn resize_widgets(&mut self) {
        if let Some(layer) = &self.widgets_surface {
            layer.set_size(widgets::W, widgets::height(self.weather.is_some()));
            layer.wl_surface().commit();
        }
    }

    pub fn create_dock(&mut self, qh: &QueueHandle<Self>) {
        let surface = self.compositor_state.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("cosmos-dock"),
            None,
        );
        let (sw, sh) = dock::surface_size(self.dock_position);
        match self.dock_position {
            dock::DockPos::Left => layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT),
            dock::DockPos::Right => layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT),
            dock::DockPos::Bottom => {
                layer.set_anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
            }
        }
        layer.set_size(sw, sh);
        layer.set_exclusive_zone((dock::STRIP + dock::MARGIN) as i32);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.wl_surface().commit();
        self.dock_surface = Some(layer);
        self.dock_dirty = true;
    }

    /// Repaint the dock after the window/app set or hover changed.
    /// (Rail size is configure-driven — the surface spans the edge —
    /// so this never resizes, just repaints.)
    pub fn sync_dock(&mut self) {
        if self.dock_surface.is_some() {
            self.dock_dirty = true;
        }
    }

    /// Toggle the keybind cheatsheet (super+?).
    pub fn set_help_open(&mut self, open: bool) {
        if open == self.help_open {
            return;
        }
        self.help_open = open;
        if open {
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("cosmos-help"),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.wl_surface().commit();
            self.help_surface = Some(layer);
            self.help_dirty = true;
        } else {
            self.help_surface = None;
        }
    }

    /// Shell-initiated close — keep the compositor's `help_open` flag
    /// in sync or the next super+? lands on a stale toggle.
    fn close_help(&mut self) {
        if self.help_open {
            self.set_help_open(false);
            self.ipc.send(&cosmos_ipc::Request::ToggleHelp);
        }
    }

    /// Toggle the dynamic island card — the menubar pill's expanded
    /// surface. It sizes itself to its content (clipboard history +
    /// staged files) each time it opens.
    pub fn set_island_open(&mut self, open: bool) {
        if open == self.island_open {
            return;
        }
        self.island_open = open;
        let animate = !LITE.with(|l| l.get());
        if open && self.island_surface.is_some() {
            // Re-opened mid reverse-morph: turn the running morph around.
            self.start_island_anim(true);
            return;
        }
        if open {
            self.island_tab = if matches!(island::pill(self), island::Pill::Idle)
                && !self.clip_history.is_empty()
            {
                island::Tab::Clipboard
            } else {
                island::Tab::Activities
            };
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Top,
                Some("cosmos-island"),
                None,
            );
            layer.set_anchor(Anchor::TOP);
            layer.set_size(island::CARD_W, island::card_height(self));
            layer.set_exclusive_zone(0);
            layer.set_margin((PANEL_HEIGHT + 4) as i32, 0, 0, 0);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.wl_surface().commit();
            self.island_surface = Some(layer);
            self.island_dirty = true;
            if animate {
                self.island_anim = None;
                self.start_island_anim(true);
            }
        } else if animate && self.island_surface.is_some() {
            self.island_hover = None;
            self.start_island_anim(false);
        } else {
            self.island_surface = None;
            self.island_hover = None;
            self.island_anim = None;
        }
    }

    /// Start (or reverse) the 280ms capsule↔card morph; a ~60fps timer
    /// repaints the card until it ends, then drops a closed card's surface.
    fn start_island_anim(&mut self, opening: bool) {
        let now = std::time::Instant::now();
        let start = match self.island_anim {
            Some((t0, _)) => {
                let done = (t0.elapsed().as_secs_f32() / island::MORPH.as_secs_f32()).min(1.0);
                now - island::MORPH.mul_f32(1.0 - done)
            }
            None => now,
        };
        self.island_anim = Some((start, opening));
        self.island_dirty = true;
        if self.island_anim_timer {
            return;
        }
        self.island_anim_timer = true;
        let _ = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_millis(16)),
            |_, _, state| {
                state.island_dirty = true;
                let Some((t0, opening)) = state.island_anim else {
                    state.island_anim_timer = false;
                    return calloop::timer::TimeoutAction::Drop;
                };
                if t0.elapsed() < island::MORPH {
                    return calloop::timer::TimeoutAction::ToDuration(Duration::from_millis(16));
                }
                state.island_anim = None;
                state.island_anim_timer = false;
                if !opening {
                    state.island_surface = None;
                }
                calloop::timer::TimeoutAction::Drop
            },
        );
    }

    /// Shell-side close — no IPC round trip needed: the compositor just
    /// routes presses to whatever is under the pointer.
    pub fn close_island(&mut self) {
        self.set_island_open(false);
    }

    /// Re-size and repaint an open island card (its live rows change).
    pub fn refresh_island(&mut self) {
        if let Some(layer) = &self.island_surface {
            layer.set_size(island::CARD_W, island::card_height(self));
            self.island_dirty = true;
        }
    }

    /// Click inside the island card.
    pub fn island_click(&mut self, x: f64, y: f64) {
        match island::hit_test(self, x, y) {
            island::Hit::Tab(t) => {
                self.island_tab = t;
                self.island_dirty = true;
            }
            island::Hit::Approve(i, k) => {
                let picked = activity::pending_approvals(&self.notifications)
                    .get(i)
                    .and_then(|n| n.actions.get(k).map(|(key, _)| (n.id, key.clone())));
                if let Some((id, key)) = picked {
                    if let Some(conn) = &self.notify_conn {
                        notify::emit_action(conn, id, &key);
                    }
                    self.notifications.retain(|n| n.id != id);
                    self.notify_dirty = true;
                    self.sync_notify_height();
                }
                self.panel_dirty = true;
                self.refresh_island();
            }
            island::Hit::AgentPause(i) => {
                if let Some(a) = self.agents.get(i) {
                    activity::set_paused(&a.agent, !a.paused);
                }
                self.agents = activity::scan_agents();
                self.panel_dirty = true;
                self.refresh_island();
            }
            island::Hit::AgentStop(i) => {
                if let Some(a) = self.agents.get(i) {
                    activity::set_stopped(&a.agent);
                }
                self.agents = activity::scan_agents();
                self.panel_dirty = true;
                self.refresh_island();
            }
            island::Hit::Media => {
                if let Some(m) = self.media.as_mut() {
                    activity::play_pause(m.bus.clone());
                    m.playing = !m.playing;
                }
                self.panel_dirty = true;
                self.refresh_island();
            }
            island::Hit::Clip(i) => {
                if let Some(text) = self.clip_history.get(i).cloned() {
                    clipwatch::set_clipboard(self, text);
                    self.close_island();
                }
            }
            island::Hit::Chip(i) => {
                // A staged file's path goes back on the clipboard.
                if let Some(path) = self.staged_files.get(i).cloned() {
                    clipwatch::set_clipboard(self, path);
                }
            }
            island::Hit::Backdrop => {}
        }
    }

    /// Toggle the quick-settings flyout (Win11-style tray popover).
    /// Flip Lite Mode locally and repaint every glass surface.
    pub fn set_lite(&mut self, on: bool) {
        self.lite = on;
        LITE.with(|l| l.set(on));
        self.panel_dirty = true;
        self.dock_dirty = true;
        self.launcher_dirty = true;
        self.notify_dirty = true;
        self.quick_dirty = true;
        self.switcher_dirty = true;
    }

    pub fn set_quick_open(&mut self, open: bool) {
        self.cc_detail = None;
        if open == self.quick_open {
            return;
        }
        self.quick_open = open;
        if open {
            self.vol_drag = false;
            self.bright_drag = false;
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Top,
                Some("cosmos-quick"),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
            layer.set_size(quick::QUICK_W, quick::desired_height(self));
            layer.set_exclusive_zone(0);
            layer.set_margin((PANEL_HEIGHT + 4) as i32, 8, 0, 0);
            // Exclusive keyboard interactivity is what makes the flyout
            // dismissable: it takes focus on map, and any click landing on
            // a window/panel pulls focus away — the keyboard `leave` then
            // closes it, matching Win11/macOS popover behavior.
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.wl_surface().commit();
            self.quick_surface = Some(layer);
            self.quick_dirty = true;
        } else {
            self.quick_surface = None;
            self.vol_drag = false;
            self.bright_drag = false;
        }
    }

    /// Click inside the quick-settings card.
    pub fn quick_click(&mut self, x: f64, y: f64) {
        self.quick_dirty = quick::press(self, x, y) || self.quick_dirty;
    }

    /// Focus mode (DND): hide/show the notification surface and repaint
    /// the tray moon + the Control Centre card itself.
    pub fn set_dnd(&mut self, on: bool) {
        if self.dnd == on {
            return;
        }
        self.dnd = on;
        self.sync_notify_surface();
        self.panel_dirty = true;
        self.quick_dirty = true;
    }

    pub fn set_launcher_open(&mut self, open: bool) {
        if open == self.launcher_open {
            return;
        }
        self.launcher_open = open;
        self.start_todo_edit = None;
        if open {
            self.set_quick_open(false);
            self.launcher_query = self.launcher_preset.take().unwrap_or_default();
            self.launcher_sel = 0;
            self.launcher_hover = None;
            fileindex::maybe_refresh(self);
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("cosmos-launcher"),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.wl_surface().commit();
            self.launcher_surface = Some(layer);
            self.launcher_dirty = true;
        } else {
            self.launcher_surface = None;
            self.launcher_search = false;
            self.launcher_preview = None;
            self.cancel_ask();
        }
    }

    /// Unified Search-or-Ask rows for the current query (apps, files,
    /// calculator, commands, Ask) — see `search::results`.
    pub fn filtered_results(&self) -> Vec<search::Row> {
        search::results(self)
    }

    pub fn filtered_apps(&self) -> Vec<&AppEntry> {
        let q = self.launcher_query.to_lowercase();
        self.apps
            .iter()
            .filter(|a| {
                q.is_empty()
                    || a.name.to_lowercase().contains(&q)
                    || a.id.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Record an app launch into the MRU (RECOMMENDED section source).
    pub fn record_launch(&mut self, id: &str) {
        self.recent.retain(|x| x != id);
        self.recent.insert(0, id.to_string());
        self.recent.truncate(8);
        launcher::save_recent(&self.recent);
    }

    /// Close the launcher on the shell's own initiative (backdrop click,
    /// item launch, footer action). The compositor still believes the
    /// launcher is open after a local close — `launcher_open` only flips
    /// on `ToggleLauncher` — so notify it, or the NEXT toggle request
    /// (dock glyph, Super) is silently eaten and the launcher looks stuck.
    /// Stream an answer to `q` into the Search or Ask card.
    fn start_ask(&mut self, q: String) {
        self.cancel_ask();
        self.ask_id += 1;
        self.ask_query = q.trim().to_string();
        self.ask_text.clear();
        self.ask_done = None;
        if let Some(tx) = &self.ask_tx {
            ask::start(self.ask_query.clone(), self.ask_id, tx.clone());
        }
        self.launcher_dirty = true;
    }

    fn cancel_ask(&mut self) {
        if let Some(pid) = self.ask_pid.take() {
            // SAFETY: plain kill(2) on the opencode child we spawned.
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
        self.ask_id += 1;
        self.ask_query.clear();
    }

    fn close_launcher(&mut self) {
        self.start_todo_edit = None;
        if self.launcher_open {
            self.set_launcher_open(false);
            self.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
        }
    }

    /// Pointer click inside the launcher surface.
    pub fn launcher_click(&mut self, x: f64, y: f64) {
        if self.launcher_search {
            let rows = self.filtered_results();
            let empty = self.launcher_query.is_empty();
            match launcher::hit_test_search(
                x,
                y,
                self.launcher_size,
                &rows,
                empty,
                self.launcher_card_h,
            ) {
                launcher::Hit::Item(idx) => {
                    self.launcher_sel = idx;
                    self.launch_selected();
                }
                launcher::Hit::Backdrop => self.close_launcher(),
                _ => {}
            }
            return;
        }
        let np = launcher::pinned(self).len();
        let n_items = launcher::rows_shown(
            !self.launcher_query.is_empty(),
            np,
            self.filtered_results().len(),
        );
        let hit = launcher::hit_test(
            x,
            y,
            self.launcher_size,
            n_items,
            np,
            launcher::recommended(self).len(),
            !self.launcher_query.is_empty(),
            self.dock_position,
        );
        match hit {
            launcher::Hit::Item(idx) => {
                self.launcher_sel = idx;
                self.launch_selected();
            }
            launcher::Hit::Cell(idx) => {
                let app = launcher::pinned(self).get(idx).cloned();
                if let Some(app) = app {
                    if let Err(err) = desktop::launch(&app) {
                        tracing::warn!("launch {} failed: {err}", app.id);
                    } else {
                        self.record_launch(&app.id);
                        self.close_launcher();
                    }
                }
            }
            launcher::Hit::Recent(idx) => {
                let app = launcher::recommended(self).get(idx).cloned();
                if let Some(app) = app {
                    if let Err(err) = desktop::launch(&app) {
                        tracing::warn!("launch {} failed: {err}", app.id);
                    } else {
                        self.record_launch(&app.id);
                        self.close_launcher();
                    }
                }
            }
            launcher::Hit::Action(0) => {
                let _ = std::process::Command::new("cosmos-settings").spawn();
                self.record_launch("cosmos-settings");
                self.close_launcher();
            }
            launcher::Hit::Action(1) => {
                if let Some(app) = self.apps.iter().find(|a| a.id == "sys.logout").cloned() {
                    let _ = desktop::launch(&app);
                }
                self.close_launcher();
            }
            launcher::Hit::Widget(dx, dy) => {
                let w = LAUNCHER_WIDTH as f32 - 32.0;
                if startw::press(self, w, dx as f32, dy as f32) {
                    self.close_launcher();
                }
                self.launcher_dirty = true;
            }
            launcher::Hit::Action(_) | launcher::Hit::Input | launcher::Hit::List => {}
            launcher::Hit::Backdrop => self.close_launcher(),
        }
    }

    pub fn launch_selected(&mut self) {
        let rows = self.filtered_results();
        let idx = self.launcher_sel.min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(idx).cloned() else {
            return;
        };
        match row.kind {
            search::Kind::App(app) => {
                if let Err(err) = desktop::launch(&app) {
                    tracing::warn!("launch {} failed: {err}", app.id);
                } else {
                    self.record_launch(&app.id);
                }
            }
            search::Kind::Calc(v) => {
                clipwatch::set_clipboard(self, v);
            }
            search::Kind::Cmd(cmd) => {
                spawn_quiet("cosmos-terminal", &["-e", &cmd]);
            }
            search::Kind::File(path) => {
                // xdg-open resolves the right app; the file manager is
                // the deterministic fallback inside its parent dir.
                let opened = std::process::Command::new("xdg-open")
                    .arg(&path)
                    .spawn()
                    .is_ok();
                if !opened {
                    let dir = path
                        .rsplit_once('/')
                        .map(|(d, _)| d)
                        .unwrap_or("/")
                        .to_string();
                    spawn_quiet("cosmos-files", &[&dir]);
                }
            }
            search::Kind::Setting(pane) => {
                spawn_quiet("cosmos-settings", &["--pane", pane]);
                self.record_launch("cosmos-settings");
            }
            search::Kind::Ask(q) if self.launcher_search => {
                self.start_ask(q);
                return;
            }
            search::Kind::Ask(q) => {
                let cmd = format!("opencode run {}", search::shell_quote(&q));
                spawn_quiet("cosmos-terminal", &["-e", &cmd]);
            }
        }
        self.close_launcher();
    }

    /// Snap Assist picker on the free half. `fill_left` = the picker
    /// occupies the left half (the snap went right).
    pub fn set_assist(&mut self, open: bool, fill_left: bool, ids: Vec<u64>) {
        if !open {
            self.assist_open = false;
            self.assist_surface = None;
            self.assist_ids.clear();
            return;
        }
        self.assist_fill_left = fill_left;
        self.assist_ids = ids;
        self.assist_sel = 0;
        self.assist_hover = None;
        if self.assist_surface.is_none() {
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("cosmos-assist"),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.wl_surface().commit();
            self.assist_surface = Some(layer);
        }
        self.assist_open = true;
        self.assist_dirty = true;
    }

    /// Close the assist on the shell's own initiative (backdrop, Esc).
    /// Mirrors `close_launcher`: the compositor still believes the
    /// picker is open — without a Dismiss the assist state lingers and
    /// the next snap offer would desync.
    pub fn close_assist(&mut self) {
        if self.assist_open {
            self.assist_open = false;
            self.assist_surface = None;
            self.assist_ids.clear();
            self.ipc.send(&cosmos_ipc::Request::SnapAssistDismiss);
        }
    }

    /// Enter/click on a row: snap that window into the free half.
    pub fn assist_pick_selected(&mut self) {
        let idx = self.assist_sel.min(self.assist_ids.len().saturating_sub(1));
        let Some(&id) = self.assist_ids.get(idx) else {
            return;
        };
        self.assist_open = false;
        self.assist_surface = None;
        self.assist_ids.clear();
        self.ipc.send(&cosmos_ipc::Request::SnapAssistPick { id });
    }

    pub fn assist_click(&mut self, x: f64, y: f64) {
        match assist::hit_test(self, x, y) {
            assist::Hit::Row(i) => {
                self.assist_sel = i;
                self.assist_pick_selected();
            }
            assist::Hit::Backdrop => self.close_assist(),
        }
    }

    /// Zoom flyout — Win11's snap-layouts card under the green zoom
    /// button. `x,y` is the button position the compositor broadcast;
    /// the card centres under it, clamped on-screen.
    pub fn set_zoom(&mut self, open: bool, window: u64, x: i32, y: i32) {
        if !open {
            self.zoom_surface = None;
            self.zoom_hover = None;
            return;
        }
        self.zoom_window = window;
        self.zoom_hover = None;
        let (cw, ch) = zoomflyout::card_size();
        // Centre the card under the button, clamped to the screen.
        let (screen_w, _screen_h) = self.panel_size;
        let left = (x - cw as i32 / 2).clamp(4, (screen_w as i32 - cw as i32 - 4).max(4));
        if self.zoom_surface.is_none() {
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("cosmos-zoomflyout"),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_size(cw, ch);
            layer.set_margin(y, 0, 0, left);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.wl_surface().commit();
            self.zoom_surface = Some(layer);
        }
        self.zoom_dirty = true;
    }

    /// Open menubar menu `menu` with its card's left edge at screen `x`
    /// (None closes). Recreated per open so the position is exact.
    pub fn set_menu(&mut self, menu: Option<usize>, x: i32) {
        self.set_menu_at(menu, x, None);
    }

    /// Desktop context menu at the press point, kept on screen.
    pub fn open_desktop_menu(&mut self, x: i32, y: i32) {
        self.close_launcher();
        self.set_quick_open(false);
        self.set_menu_at(Some(menubar::DESKTOP), x, Some(y));
    }

    /// `y: None` drops the card just under the menubar; `Some(y)` puts
    /// its top edge at screen `y`.
    fn set_menu_at(&mut self, menu: Option<usize>, x: i32, y: Option<i32>) {
        self.menu_surface = None;
        self.menu_hover = None;
        self.menu_open = menu;
        let Some(m) = menu else {
            self.panel_dirty = true;
            return;
        };
        let (w, h) = menubar::surface_size(m);
        let inset = menubar::INSET as i32;
        let left = (x - inset).clamp(0, (self.panel_size.0 as i32 - w as i32).max(0));
        let top = match y {
            Some(y) => {
                let sh = glass::screen_size().1 as i32;
                (y - inset).clamp(0, (sh - h as i32).max(0))
            }
            None => self.panel_size.1 as i32 + 2 - inset,
        };
        self.menu_pos = (left, top);
        let surface = self.compositor_state.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("cosmos-menu"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_size(w, h);
        layer.set_margin(top, 0, 0, left);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer.wl_surface().commit();
        self.menu_surface = Some(layer);
        self.menu_dirty = true;
        self.panel_dirty = true;
    }

    pub fn menu_click(&mut self, x: f64, y: f64) {
        let Some(m) = self.menu_open else { return };
        let Some(row) = menubar::item_at(m, x, y) else {
            return;
        };
        let action = menubar::items(m)[row].1;
        let focused = self.windows.iter().find(|w| w.focused).cloned();
        if action.needs_window() && focused.is_none() {
            return;
        }
        self.set_menu(None, 0);
        use menubar::Action;
        let id = focused.as_ref().map(|w| w.id).unwrap_or(0);
        let snap = |zone: &str| cosmos_ipc::Request::SnapToZone {
            id,
            zone: zone.to_string(),
        };
        match action {
            Action::NewWindow => {
                let key = focused.as_ref().map(|w| icons::key_for(&w.app_id));
                if let Some(app) = self
                    .apps
                    .iter()
                    .find(|a| Some(&a.id) == key.as_ref())
                    .cloned()
                {
                    if let Err(err) = desktop::launch(&app) {
                        tracing::warn!("menu: launch {} failed: {err}", app.id);
                    }
                }
            }
            Action::Close => self.ipc.send(&cosmos_ipc::Request::CloseWindow { id }),
            Action::Minimize => self.ipc.send(&cosmos_ipc::Request::MinimizeWindow { id }),
            Action::Zoom => self.ipc.send(&snap("max")),
            Action::TileLeft => self.ipc.send(&snap("left")),
            Action::TileRight => self.ipc.send(&snap("right")),
            Action::NextWorkspace => {
                let count = workspace_count(self).max(1);
                let cur = self
                    .workspaces
                    .iter()
                    .find(|w| w.focused)
                    .map(|w| w.id as usize)
                    .unwrap_or(0);
                self.ipc.send(&cosmos_ipc::Request::MoveWindowToWorkspace {
                    id,
                    workspace: ((cur + 1) % count) as u8,
                });
            }
            Action::Shortcuts => self.ipc.send(&cosmos_ipc::Request::ToggleHelp),
            Action::Search => self.ipc.send(&cosmos_ipc::Request::ToggleLauncher),
            Action::Settings => {
                let _ = std::process::Command::new("cosmos-settings").spawn();
            }
            Action::Lock => {
                if let Err(err) = logind::request_lock() {
                    tracing::warn!("menu: lock failed: {err}");
                }
            }
            Action::ChangeWallpaper => spawn_logged("cosmos-settings", &["--pane", "wallpaper"]),
            Action::ViewOptions => spawn_logged("cosmos-settings", &["--pane", "dock"]),
            Action::NewFolder => match new_desktop_folder() {
                Ok(dir) => spawn_logged("cosmos-files", &[&dir.to_string_lossy()]),
                Err(err) => tracing::warn!("menu: new folder failed: {err}"),
            },
            Action::AskCosmos => {
                self.launcher_preset = Some("?".to_string());
                self.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
            }
            Action::TerminalHere => {
                let dir = desktop_dir();
                let _ = std::fs::create_dir_all(&dir);
                if let Err(err) = std::process::Command::new("cosmos-terminal")
                    .current_dir(&dir)
                    .spawn()
                {
                    tracing::warn!("menu: terminal failed: {err}");
                }
            }
            Action::Logout | Action::Restart | Action::Shutdown => {
                let key = match action {
                    Action::Logout => "sys.logout",
                    Action::Restart => "sys.reboot",
                    _ => "sys.shutdown",
                };
                if let Some(app) = self.apps.iter().find(|a| a.id == key).cloned() {
                    if let Err(err) = desktop::launch(&app) {
                        tracing::warn!("menu: {key} failed: {err}");
                    }
                }
            }
        }
    }

    /// Close the flyout on the shell's own initiative (Esc). Outside
    /// presses already miss this surface — the compositor sees them,
    /// closes its flag, and broadcasts `ZoomFlyout{open:false}` which
    /// drops the surface through `set_zoom`. This path is only for
    /// keys we still own focus for.
    pub fn close_zoom(&mut self) {
        if self.zoom_surface.is_some() {
            self.zoom_surface = None;
            self.zoom_hover = None;
            self.ipc.send(&cosmos_ipc::Request::ZoomFlyoutDismiss);
        }
    }

    /// Click on a zone cell → snap the anchor window there.
    pub fn zoom_click(&mut self, x: f64, y: f64) {
        if let zoomflyout::Hit::Zone(li, ci) = zoomflyout::hit_test(x, y) {
            if let Some(zone) = zoomflyout::zone_name(li, ci) {
                let id = self.zoom_window;
                self.zoom_surface = None;
                self.zoom_hover = None;
                self.ipc.send(&cosmos_ipc::Request::SnapToZone {
                    id,
                    zone: zone.to_string(),
                });
            }
        }
    }

    /// Centered Alt/Super+Tab overlay — created on the first `Switcher`
    /// event and dropped when the compositor reports `open: false`.
    fn create_switcher(&mut self, qh: &QueueHandle<Self>) {
        let surface = self.compositor_state.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("cosmos-switcher"),
            None,
        );
        // No anchors: the compositor centers the surface on both axes.
        layer.set_anchor(Anchor::empty());
        layer.set_size(
            switcher::desired_width(self.switcher_order.len()),
            switcher::SWITCHER_H,
        );
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        // Empty input region: the overlay never eats pointer events.
        layer.wl_surface().set_input_region(None);
        layer.wl_surface().commit();
        self.switcher_size = (
            switcher::desired_width(self.switcher_order.len()),
            switcher::SWITCHER_H,
        );
        self.switcher_surface = Some(layer);
        self.switcher_dirty = true;
    }

    /// Bring the switcher surface in sync with the latest `Switcher` event.
    fn sync_switcher(&mut self) {
        if self.switcher_surface.is_none() {
            let qh = self.qh.clone();
            self.create_switcher(&qh);
            return;
        }
        let want_w = switcher::desired_width(self.switcher_order.len());
        if let Some(layer) = &self.switcher_surface {
            if self.switcher_size.0 != want_w {
                layer.set_size(want_w, switcher::SWITCHER_H);
                layer.wl_surface().commit();
            }
        }
        self.switcher_dirty = true;
    }

    fn on_ipc_event(&mut self, ev: cosmos_ipc::Event) {
        use cosmos_ipc::Event::*;
        match ev {
            Windows { windows } => {
                self.windows = windows;
                self.panel_dirty = true;
                self.sync_dock();
            }
            Workspaces { workspaces } => {
                self.workspaces = workspaces;
                self.panel_dirty = true;
            }
            LauncherToggled { open, search } => {
                if open && !self.launcher_open {
                    self.launcher_search = search || self.launcher_preset.is_some();
                }
                self.set_launcher_open(open);
            }
            HelpToggled { open } => self.set_help_open(open),
            SnapAssist {
                open,
                fill,
                candidates,
            } => {
                if open {
                    // One overlay at a time — a launcher left open would
                    // hide the picker and desync its compositor flag.
                    self.close_launcher();
                    self.set_assist(true, fill == "left", candidates);
                } else {
                    self.set_assist(false, false, Vec::new());
                }
            }
            ZoomFlyout { open, window, x, y } => {
                self.set_zoom(open, window, x, y);
            }
            DesktopPress { button, x, y } => {
                const BTN_RIGHT: u32 = 0x111;
                self.set_menu(None, 0);
                self.set_quick_open(false);
                if button == BTN_RIGHT {
                    self.open_desktop_menu(x, y);
                }
            }
            Switcher {
                open,
                selected,
                order,
            } => {
                if open {
                    self.switcher_sel = selected;
                    self.switcher_order = order;
                    self.sync_switcher();
                } else {
                    self.switcher_surface = None;
                    self.switcher_order.clear();
                }
            }
            Config(map) => self.apply_config(map),
            SessionEnding => self.exit = true,
            Pong { .. } | Screenshot { .. } | Error { .. } => {}
        }
    }

    /// The tray's quick-settings click region — rightmost status text block.
    pub fn tray_clicked(&mut self, x: f64) -> bool {
        let (w, _) = self.panel_size;
        let status_w = crate::panel::status_text_len(&self.sysinfo, self.dnd);
        x >= w as f64 - status_w - 12.0
    }

    fn apply_config(&mut self, map: serde_json::Map<String, serde_json::Value>) {
        // Paint at the output scale; without a viewporter the buffer
        // would set the surface size, so stay at 1×.
        if let Some(s) = map.get("scale").and_then(|v| v.as_f64()) {
            let s = if self.viewporter.is_some() {
                s as f32
            } else {
                1.0
            };
            if (s - draw::scale()).abs() > f32::EPSILON {
                tracing::info!(scale = s, "shell: output scale");
                draw::set_scale(s);
                self.panel_dirty = true;
                self.dock_dirty = true;
                self.launcher_dirty = true;
                self.notify_dirty = true;
                self.switcher_dirty = true;
            }
        }
        if let Some(v) = map.get("appearance").and_then(|v| v.as_str()) {
            self.dark = v == "dark";
            DARK.with(|d| d.set(self.dark));
            self.panel_dirty = true;
            self.dock_dirty = true;
            self.launcher_dirty = true;
            self.notify_dirty = true;
        }
        if let Some(on) = map.get("lite_mode").and_then(|v| v.as_bool()) {
            if on != self.lite {
                self.set_lite(on);
            }
        }
        if let Some(v) = map.get("accent").and_then(|v| v.as_str()) {
            self.accent_name = v.to_string();
        }
        if let Some(v) = map.get("wallpaper").and_then(|v| v.as_str()) {
            if v != self.wallpaper {
                self.wallpaper = v.to_string();
                WALLPAPER.with(|w| *w.borrow_mut() = v.to_string());
                self.panel_dirty = true;
                self.dock_dirty = true;
            }
        }
        if let Some(on) = map.get("desktop_widgets").and_then(|v| v.as_bool()) {
            if on != self.show_widgets {
                self.show_widgets = on;
                if on {
                    let qh = self.qh.clone();
                    self.create_widgets(&qh);
                } else {
                    self.widgets_surface = None;
                }
            }
        }
        if let Some(city) = map.get("weather_city").and_then(|v| v.as_str()) {
            if city != self.weather_city {
                self.weather_city = city.to_string();
                if let Some(tx) = &self.weather_tx {
                    let _ = tx.send(city.to_string());
                }
            }
        }
        // Dock edge — a position change re-anchors the surface entirely
        // (anchors + exclusive zone), so the layer is recreated.
        if let Some(pos) = map
            .get("dock_position")
            .and_then(|v| v.as_str())
            .and_then(dock::DockPos::parse)
        {
            if pos != self.dock_position {
                self.dock_position = pos;
                self.dock_surface = None;
                self.widgets_dirty = true;
                let qh = self.qh.clone();
                self.create_dock(&qh);
            }
        }
        // The compositor resolves the accent preset and ships both rgbs
        // in the config map — adopt them into the draw helpers.
        let rgb = |key: &str| {
            map.get("accent_rgb")
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_array())
                .filter(|a| a.len() == 3)
                .map(|a| {
                    [
                        a[0].as_u64().unwrap_or(0) as u8,
                        a[1].as_u64().unwrap_or(0) as u8,
                        a[2].as_u64().unwrap_or(0) as u8,
                    ]
                })
        };
        if let (Some(d), Some(l)) = (rgb("dark"), rgb("light")) {
            draw::set_accent(d, l);
            self.panel_dirty = true;
            self.launcher_dirty = true;
            self.notify_dirty = true;
            self.quick_dirty = true;
            self.dock_dirty = true;
            self.switcher_dirty = true;
        }
    }

    fn on_notify(&mut self, ev: NotifyEvent) {
        match ev {
            NotifyEvent::Raised(n) => {
                if let Some(pos) = self.notifications.iter().position(|x| x.id == n.id) {
                    self.notifications[pos] = n;
                } else {
                    self.notifications.push(n);
                }
                self.notify_dirty = true;
            }
            NotifyEvent::Closed { id, .. } => {
                self.notifications.retain(|n| n.id != id);
                self.notify_dirty = true;
            }
        }
        self.sync_notify_surface();
        self.sync_notify_height();
    }

    fn on_tick(&mut self) {
        self.panel_dirty = true; // clock
        self.agents = activity::scan_agents();
        // Elapsed timers + approval/agent changes.
        self.refresh_island();
        let before = self.notifications.len();
        let now = std::time::Instant::now();
        self.notifications.retain(|n| {
            if n.timeout_ms < 0 {
                return true; // persistent — approval cards wait for a decision
            }
            let timeout = if n.timeout_ms == 0 {
                NOTIFY_TIMEOUT_MS
            } else {
                n.timeout_ms as i64
            };
            now.duration_since(n.arrived).as_millis() < timeout as u128
        });
        if self.notifications.len() != before {
            self.notify_dirty = true;
        }
        self.sync_notify_surface();

        // IPC reconnect on a 5s cadence.
        if !self.ipc.connected() {
            self.ipc_retry_in = self.ipc_retry_in.saturating_sub(1);
            if self.ipc_retry_in == 0 {
                self.ipc_retry_in = 5;
                if let Some(reader) = self.ipc.connect() {
                    tracing::info!("reconnected to compositor IPC");
                    register_ipc_source(&self.loop_handle, reader);
                }
            }
        }
    }

    fn sync_notify_surface(&mut self) {
        // Focus hides banners — but urgency=critical (approval cards) always shows.
        let visible = !self.dnd || self.notifications.iter().any(|n| n.critical);
        if (self.notifications.is_empty() || !visible) && self.notify_surface.is_some() {
            self.notify_surface = None;
            return;
        }
        if !self.notifications.is_empty() && visible && self.notify_surface.is_none() {
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("cosmos-notify"),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
            layer.set_size(NOTIFY_WIDTH, 1);
            layer.set_exclusive_zone(0);
            layer.set_margin((PANEL_HEIGHT + 8) as i32, 8, 0, 0);
            layer.wl_surface().commit();
            self.notify_surface = Some(layer);
        }
    }

    /// Draw whichever surfaces are dirty.
    pub fn render(&mut self) {
        if self.panel_dirty && self.panel.is_some() {
            self.panel_dirty = false;
            panel::draw(self);
        }
        if self.launcher_dirty && self.launcher_surface.is_some() {
            self.launcher_dirty = false;
            launcher::draw(self);
        }
        if self.notify_dirty && self.notify_surface.is_some() {
            self.notify_dirty = false;
            popups::draw(self);
        }
        if self.quick_dirty && self.quick_surface.is_some() {
            self.quick_dirty = false;
            quick::draw(self);
        }
        if self.dock_dirty && self.dock_surface.is_some() {
            self.dock_dirty = false;
            dock::draw(self);
        }
        if self.widgets_dirty && self.widgets_surface.is_some() {
            self.widgets_dirty = false;
            widgets::draw(self);
        }
        if self.switcher_dirty && self.switcher_surface.is_some() {
            self.switcher_dirty = false;
            switcher::draw(self);
        }
        if self.assist_dirty && self.assist_surface.is_some() {
            self.assist_dirty = false;
            assist::draw(self);
        }
        if self.menu_dirty && self.menu_surface.is_some() {
            self.menu_dirty = false;
            menubar::draw(self);
        }
        if self.zoom_dirty && self.zoom_surface.is_some() {
            self.zoom_dirty = false;
            zoomflyout::draw(self);
        }
        if self.help_dirty && self.help_surface.is_some() {
            self.help_dirty = false;
            help::draw(self);
        }
        if self.island_dirty && self.island_surface.is_some() {
            self.island_dirty = false;
            island::draw(self);
        }
    }
}

// ---------------------------------------------------------------- traits

impl CompositorHandler for ShellState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Frame callbacks: just draw — surfaces repaint when dirty anyway.
        let _ = qh;
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for ShellState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.panel.as_ref() == Some(layer) {
            self.panel = None;
        }
        if self.dock_surface.as_ref() == Some(layer) {
            self.dock_surface = None;
        }
        if self.launcher_surface.as_ref() == Some(layer) {
            self.launcher_surface = None;
            self.launcher_open = false;
        }
        if self.notify_surface.as_ref() == Some(layer) {
            self.notify_surface = None;
        }
        if self.quick_surface.as_ref() == Some(layer) {
            self.quick_surface = None;
            self.quick_open = false;
        }
        if self.assist_surface.as_ref() == Some(layer) {
            self.assist_surface = None;
            self.assist_open = false;
        }
        if self.zoom_surface.as_ref() == Some(layer) {
            self.zoom_surface = None;
        }
        if self.menu_surface.as_ref() == Some(layer) {
            self.menu_surface = None;
            self.menu_open = None;
        }
        if self.help_surface.as_ref() == Some(layer) {
            self.help_surface = None;
            self.help_open = false;
        }
        if self.island_surface.as_ref() == Some(layer) {
            self.island_surface = None;
            self.island_open = false;
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        serial: u32,
    ) {
        if self.panel.as_ref() == Some(layer) {
            self.panel_size = (configure.new_size.0, PANEL_HEIGHT);
            self.panel_dirty = true;
        }
        if self.dock_surface.as_ref() == Some(layer) {
            self.dock_size = configure.new_size;
            self.dock_dirty = true;
            // A side rail spans the output's height — with the panel's
            // width that's the full screen size for glass sampling (a
            // bottom dock is only its own height; xdg-output covers it).
            if self.dock_position != dock::DockPos::Bottom {
                glass::set_screen_size(self.panel_size.0 as f32, configure.new_size.1 as f32);
            }
        }
        if self.switcher_surface.as_ref() == Some(layer) {
            self.switcher_size = (
                configure
                    .new_size
                    .0
                    .max(switcher::desired_width(self.switcher_order.len())),
                switcher::SWITCHER_H,
            );
            self.switcher_dirty = true;
        }
        if self.widgets_surface.as_ref() == Some(layer) {
            self.widgets_size = configure.new_size;
            self.widgets_dirty = true;
        }
        if self.launcher_surface.as_ref() == Some(layer) {
            self.launcher_size = configure.new_size;
            self.launcher_dirty = true;
        }
        if self.notify_surface.as_ref() == Some(layer) {
            let wanted_h = popups::desired_height(&self.notifications);
            let h = if configure.new_size.1 == 0 {
                wanted_h
            } else {
                wanted_h.min(configure.new_size.1.max(1))
            };
            self.notify_size = (NOTIFY_WIDTH, h);
            if configure.new_size.1 != h {
                layer.set_size(NOTIFY_WIDTH, h);
            }
            self.notify_dirty = true;
        }
        if self.quick_surface.as_ref() == Some(layer) {
            self.quick_size = (quick::QUICK_W, quick::desired_height(self));
            self.quick_dirty = true;
        }
        if self.assist_surface.as_ref() == Some(layer) {
            self.assist_size = configure.new_size;
            self.assist_dirty = true;
        }
        if self.zoom_surface.as_ref() == Some(layer) {
            self.zoom_dirty = true;
        }
        if self.menu_surface.as_ref() == Some(layer) {
            self.menu_dirty = true;
        }
        if self.help_surface.as_ref() == Some(layer) {
            self.help_size = configure.new_size;
            self.help_dirty = true;
        }
        if self.island_surface.as_ref() == Some(layer) {
            self.island_dirty = true;
        }
        // Acking configure happens via committing the surface.
        let _ = serial;
        layer.wl_surface().commit();
    }
}

impl SeatHandler for ShellState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard => {
                if let Err(e) = self.seat_state.get_keyboard(qh, &seat, None) {
                    tracing::warn!("keyboard cap failed: {e}");
                }
            }
            Capability::Pointer => {
                if let Err(e) = self.seat_state.get_pointer(qh, &seat) {
                    tracing::warn!("pointer cap failed: {e}");
                }
                if let Some(mgr) = self.data_device_manager.as_ref() {
                    self.data_devices.push(mgr.get_data_device(qh, &seat));
                }
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl KeyboardHandler for ShellState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        // Focus pulled away from an open menubar menu → dismiss it, unless
        // the pointer is on the menu: then this leave is the press itself,
        // and dismissing here dropped the click (system menu → Lock).
        if self.menu_open.is_some()
            && self.pointer_on.as_ref() != Some(surface)
            && self
                .menu_surface
                .as_ref()
                .is_some_and(|l| l.wl_surface() == surface)
        {
            self.set_menu(None, 0);
        }
        // Focus pulled away from the quick-settings flyout → dismiss it.
        // `leave` fires for ANY surface losing focus — a click inside the
        // card also moves keyboard focus (window → flyout) and the window's
        // leave must not kill the card, so only the flyout's own leave
        // dismisses.
        if self.quick_open
            && self
                .quick_surface
                .as_ref()
                .map(|l| l.wl_surface() == surface)
                .unwrap_or(false)
        {
            self.set_quick_open(false);
            self.quick_dismissed_at = Some(std::time::Instant::now());
        }
        // Same popover contract for Snap Assist: a click landing
        // anywhere else (tray, dock, a window) pulls keyboard focus off
        // the picker — dismiss it so that click isn't swallowed.
        if self.assist_open
            && self
                .assist_surface
                .as_ref()
                .map(|l| l.wl_surface() == surface)
                .unwrap_or(false)
        {
            self.close_assist();
        }
        // Island card: same popover contract — losing keyboard focus
        // (a click landing on a window, dock, menubar) dismisses it.
        if self.island_open
            && self
                .island_surface
                .as_ref()
                .map(|l| l.wl_surface() == surface)
                .unwrap_or(false)
        {
            self.close_island();
            self.island_dismissed_at = Some(std::time::Instant::now());
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if self.help_open {
            // Esc / Return / the same chord that opened it all dismiss.
            match event.keysym {
                Keysym::Escape
                | Keysym::Return
                | Keysym::KP_Enter
                | Keysym::slash
                | Keysym::question => self.close_help(),
                _ => {}
            }
        } else if self.assist_open {
            assist::key_press(self, event);
        } else if self.menu_open.is_some() {
            if event.keysym == Keysym::Escape {
                self.set_menu(None, 0);
            }
        } else if self.zoom_surface.is_some() {
            zoomflyout::key_press(self, event);
        } else if self.island_open {
            island::key_press(self, event);
        } else if self.launcher_open {
            launcher::key_press(self, event);
        }
    }

    fn repeat_key(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.press_key(conn, qh, keyboard, serial, event);
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
    }
}

impl PointerHandler for ShellState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            match ev.kind {
                PointerEventKind::Enter { .. } => self.pointer_on = Some(ev.surface.clone()),
                PointerEventKind::Leave { .. } if self.pointer_on.as_ref() == Some(&ev.surface) => {
                    self.pointer_on = None
                }
                _ => {}
            }
            let Some(layer) = self
                .panel
                .iter()
                .chain(self.dock_surface.iter())
                .chain(self.launcher_surface.iter())
                .chain(self.assist_surface.iter())
                .chain(self.zoom_surface.iter())
                .chain(self.menu_surface.iter())
                .chain(self.help_surface.iter())
                .chain(self.island_surface.iter())
                .chain(self.notify_surface.iter())
                .chain(self.quick_surface.iter())
                .find(|l| l.wl_surface() == &ev.surface)
                .cloned()
            else {
                continue;
            };
            match ev.kind {
                PointerEventKind::Press { .. } => {
                    if self.panel.as_ref() == Some(&layer) {
                        self.panel_dirty = panel::click(self, ev.position.0, ev.position.1);
                    } else if self.dock_surface.as_ref() == Some(&layer) {
                        self.dock_dirty = dock::click(self, ev.position.0, ev.position.1);
                    } else if self.launcher_surface.as_ref() == Some(&layer) {
                        self.launcher_click(ev.position.0, ev.position.1);
                    } else if self.notify_surface.as_ref() == Some(&layer) {
                        match popups::click(self, ev.position.0, ev.position.1) {
                            popups::Click::Dismissed(id) => {
                                if let Some(conn) = &self.notify_conn {
                                    notify::emit_closed(conn, id, 2);
                                }
                            }
                            popups::Click::Actioned(id, key) => {
                                if let Some(conn) = &self.notify_conn {
                                    notify::emit_action(conn, id, &key);
                                }
                            }
                            popups::Click::Nothing => {}
                        }
                    } else if self.quick_surface.as_ref() == Some(&layer) {
                        self.quick_click(ev.position.0, ev.position.1);
                    } else if self.assist_surface.as_ref() == Some(&layer) {
                        self.assist_click(ev.position.0, ev.position.1);
                    } else if self.zoom_surface.as_ref() == Some(&layer) {
                        self.zoom_click(ev.position.0, ev.position.1);
                    } else if self.menu_surface.as_ref() == Some(&layer) {
                        self.menu_click(ev.position.0, ev.position.1);
                    } else if self.help_surface.as_ref() == Some(&layer) {
                        // Any press on the sheet dismisses it.
                        self.close_help();
                    } else if self.island_surface.as_ref() == Some(&layer) {
                        self.island_click(ev.position.0, ev.position.1);
                    }
                }
                PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                    if self.panel.as_ref() == Some(&layer) {
                        self.panel_dirty |= panel::hover(self, ev.position.0, ev.position.1);
                    } else if self.dock_surface.as_ref() == Some(&layer) {
                        self.dock_dirty |= dock::hover(self, ev.position.0, ev.position.1);
                    } else if self.launcher_surface.as_ref() == Some(&layer) {
                        self.launcher_dirty |= launcher::hover(self, ev.position.0, ev.position.1);
                    } else if self.quick_surface.as_ref() == Some(&layer)
                        && (self.vol_drag || self.bright_drag)
                    {
                        self.quick_dirty |= quick::drag(self, ev.position.0, ev.position.1);
                    } else if self.assist_surface.as_ref() == Some(&layer) {
                        self.assist_dirty |= assist::hover(self, ev.position.0, ev.position.1);
                    } else if self.zoom_surface.as_ref() == Some(&layer) {
                        self.zoom_dirty |= zoomflyout::hover(self, ev.position.0, ev.position.1);
                    } else if self.menu_surface.as_ref() == Some(&layer) {
                        self.menu_dirty |= menubar::hover(self, ev.position.0, ev.position.1);
                    } else if self.island_surface.as_ref() == Some(&layer) {
                        self.island_dirty |= island::hover(self, ev.position.0, ev.position.1);
                    }
                }
                PointerEventKind::Release { .. } => {
                    if self.quick_surface.as_ref() == Some(&layer) {
                        quick::release(self);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.dock_surface.as_ref() == Some(&layer) {
                        self.dock_dirty |= dock::leave(self);
                    } else if self.panel.as_ref() == Some(&layer) {
                        self.panel_hover = (0.0, false);
                        self.panel_dirty = true;
                    } else if self.launcher_surface.as_ref() == Some(&layer)
                        && self.launcher_hover.is_some()
                    {
                        self.launcher_hover = None;
                        self.launcher_dirty = true;
                    } else if self.assist_surface.as_ref() == Some(&layer)
                        && self.assist_hover.is_some()
                    {
                        self.assist_hover = None;
                        self.assist_dirty = true;
                    } else if self.menu_surface.as_ref() == Some(&layer)
                        && self.menu_hover.is_some()
                    {
                        self.menu_hover = None;
                        self.menu_dirty = true;
                    } else if self.zoom_surface.as_ref() == Some(&layer)
                        && self.zoom_hover.is_some()
                    {
                        self.zoom_hover = None;
                        self.zoom_dirty = true;
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShellState {
    /// Glass samples the wallpaper in screen space, so it needs the
    /// output's logical size (xdg-output) — no layer surface spans it.
    fn note_output_size(&mut self, output: &wl_output::WlOutput) {
        let Some((w, h)) = self.output_state.info(output).and_then(|i| i.logical_size) else {
            return;
        };
        if w > 0 && h > 0 && glass::screen_size() != (w as f32, h as f32) {
            glass::set_screen_size(w as f32, h as f32);
            self.dock_dirty = true;
            self.panel_dirty = true;
        }
    }
}

impl OutputHandler for ShellState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.note_output_size(&output);
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.note_output_size(&output);
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl ShmHandler for ShellState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// File drops onto the island card — accept `text/uri-list` drags while
/// they hover the card and decode them into `staged_files`.
impl DataDeviceHandler for ShellState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
        _x: f64,
        _y: f64,
        surface: &wl_surface::WlSurface,
    ) {
        let on_island = self
            .island_surface
            .as_ref()
            .map(|l| l.wl_surface() == surface)
            .unwrap_or(false);
        self.dnd_on_island = on_island;
        if on_island {
            if let Some(offer) = data_device
                .data::<smithay_client_toolkit::data_device_manager::data_device::DataDeviceData>()
                .and_then(|d| d.drag_offer())
            {
                let has_uri = offer.with_mime_types(|m| m.contains(&"text/uri-list".to_string()));
                if has_uri {
                    offer.accept_mime_type(0, Some("text/uri-list".to_string()));
                }
            }
        }
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
    ) {
        self.dnd_on_island = false;
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
        _x: f64,
        _y: f64,
    ) {
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
    ) {
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
    ) {
        if !self.dnd_on_island {
            return;
        }
        let Some(offer) = data_device
            .data::<smithay_client_toolkit::data_device_manager::data_device::DataDeviceData>()
            .and_then(|d| d.drag_offer())
        else {
            return;
        };
        let Ok(mut pipe) = offer.receive("text/uri-list".to_string()) else {
            return;
        };
        offer.finish();
        let tx = self.drop_tx.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            let paths: Vec<String> = text
                .lines()
                .filter_map(|l| {
                    let l = l.trim();
                    if l.starts_with('#') || l.is_empty() {
                        return None;
                    }
                    Some(l.strip_prefix("file://").unwrap_or(l).to_string())
                })
                .collect();
            if !paths.is_empty() {
                let _ = tx.send(paths);
            }
        });
    }
}

fn spawn_quiet(cmd: &str, args: &[&str]) {
    use std::process::Stdio;
    if let Err(e) = std::process::Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        tracing::warn!("{cmd} spawn failed: {e}");
    }
}

/// Data-offer action events — DnD drops on the island don't need them.
impl DataOfferHandler for ShellState {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
}

impl ProvidesRegistryState for ShellState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers!(OutputState, SeatState);
}

smithay_client_toolkit::delegate_dispatch2!(ShellState);

impl ShellState {
    /// Map a `w`×`h` logical surface onto its logical × scale buffer.
    pub fn set_viewport(&mut self, surface: &wl_surface::WlSurface, w: u32, h: u32) {
        let Some(vp) = self.viewporter.as_ref() else {
            return;
        };
        self.viewports.retain(|(s, v)| {
            let alive = s.is_alive();
            if !alive {
                v.destroy();
            }
            alive
        });
        let idx = match self.viewports.iter().position(|(s, _)| s == surface) {
            Some(i) => i,
            None => {
                let v = vp.get_viewport(surface, &self.qh, ScaleData);
                self.viewports.push((surface.clone(), v));
                self.viewports.len() - 1
            }
        };
        self.viewports[idx].1.set_destination(w as i32, h as i32);
    }
}

/// User data for the viewporter objects.
struct ScaleData;

impl Dispatch2<WpViewporter, ShellState> for ScaleData {
    fn event(
        &self,
        _: &mut ShellState,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<ShellState>,
    ) {
    }
}

impl Dispatch2<WpViewport, ShellState> for ScaleData {
    fn event(
        &self,
        _: &mut ShellState,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<ShellState>,
    ) {
    }
}
smithay_client_toolkit::delegate_registry!(ShellState);

fn spawn_logged(cmd: &str, args: &[&str]) {
    if let Err(err) = std::process::Command::new(cmd).args(args).spawn() {
        tracing::warn!("spawn {cmd} failed: {err}");
    }
}

/// `$XDG_DESKTOP_DIR` from user-dirs.dirs, else `~/Desktop`.
fn desktop_dir() -> std::path::PathBuf {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let cfg = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| home.join(".config"));
    std::fs::read_to_string(cfg.join("user-dirs.dirs"))
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("XDG_DESKTOP_DIR="))
                .map(|v| {
                    v.trim_matches('"')
                        .replace("$HOME", &home.to_string_lossy())
                })
        })
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("Desktop"))
}

/// Create "untitled folder" (then "untitled folder 2", …) on the Desktop.
fn new_desktop_folder() -> std::io::Result<std::path::PathBuf> {
    new_folder_in(&desktop_dir())
}

fn new_folder_in(parent: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(parent)?;
    for n in 1.. {
        let name = if n == 1 {
            "untitled folder".to_string()
        } else {
            format!("untitled folder {n}")
        };
        let dir = parent.join(name);
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("1.. is unbounded")
}

#[cfg(test)]
mod desktop_menu_tests {
    #[test]
    fn new_folder_numbers_past_existing_ones() {
        let d = std::env::temp_dir().join(format!("cosmos-nf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let a = super::new_folder_in(&d).unwrap();
        let b = super::new_folder_in(&d).unwrap();
        assert_eq!(a.file_name().unwrap(), "untitled folder");
        assert_eq!(b.file_name().unwrap(), "untitled folder 2");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn desktop_menu_items_need_no_window() {
        let items = crate::menubar::items(crate::menubar::DESKTOP);
        assert_eq!(items.len(), 5);
        assert!(items.iter().all(|(_, a)| !a.needs_window()));
    }
}

#[cfg(test)]
mod wallpaper_mirror_tests {
    #[test]
    fn glass_wallpaper_defaults_to_the_shipped_default_before_any_config() {
        assert_eq!(
            super::ShellState::wallpaper_name(),
            super::DEFAULT_WALLPAPER
        );
    }
}

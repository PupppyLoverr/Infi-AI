//! cosmos-shell — the Cosmos desktop shell: top panel, app launcher,
//! notification popups. Runs as a wlr-layer-shell Wayland client and talks
//! to cosmos-compositor over cosmos-ipc.

mod assist;
mod desktop;
mod dock;
mod draw;
mod icons;
mod ipc_client;
mod launcher;
mod notify;
mod panel;
mod popups;
mod quick;
mod switcher;
mod sysinfo;

use std::{os::unix::net::UnixStream, time::Duration};

use calloop::{
    channel::{self, Event as ChannelEvent},
    generic::Generic,
    timer::Timer,
    EventLoop, Interest, LoopHandle, Mode, PostAction,
};
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
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
    Connection, QueueHandle,
};

use desktop::AppEntry;
use ipc_client::IpcClient;
use notify::{Notification, NotifyEvent};
use sysinfo::SysInfo;

pub const PANEL_HEIGHT: u32 = 32;
pub const NOTIFY_WIDTH: u32 = 340;
pub const NOTIFY_TIMEOUT_MS: i64 = 5000;
pub const LAUNCHER_WIDTH: u32 = 480;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

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

    let mut state = ShellState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        layer_shell,
        shm,
        seat_state,
        output_state,
        pool,
        qh: qh.clone(),
        loop_handle: handle.clone(),
        panel: None,
        dock_surface: None,
        switcher_surface: None,
        launcher_surface: None,
        notify_surface: None,
        quick_surface: None,
        assist_surface: None,
        notify_conn: None,
        panel_size: (0, PANEL_HEIGHT),
        dock_size: (0, dock::SURFACE_H),
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
        launcher_hover: None,
        notify_size: (0, 0),
        quick_size: (0, 0),
        windows: Vec::new(),
        workspaces: Vec::new(),
        launcher_open: false,
        launcher_query: String::new(),
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
        exit: false,
    };

    state.create_panel(&qh);
    state.create_dock(&qh);
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
    pub qh: QueueHandle<Self>,
    pub loop_handle: LoopHandle<'static, Self>,

    pub panel: Option<LayerSurface>,
    pub dock_surface: Option<LayerSurface>,
    pub switcher_surface: Option<LayerSurface>,
    pub launcher_surface: Option<LayerSurface>,
    pub notify_surface: Option<LayerSurface>,
    pub quick_surface: Option<LayerSurface>,
    /// Snap Assist picker (Win11) — fullscreen overlay on the free half.
    pub assist_surface: Option<LayerSurface>,
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
    /// Pointer hover inside the launcher card (cells, rows, footer).
    pub launcher_hover: Option<launcher::Hit>,
    pub notify_size: (u32, u32),
    pub quick_size: (u32, u32),

    pub windows: Vec<cosmos_ipc::WindowInfo>,
    pub workspaces: Vec<cosmos_ipc::WorkspaceInfo>,
    pub launcher_open: bool,
    pub launcher_query: String,
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
    /// Last pointer x on the dock (cell highlight).
    pub dock_hover: Option<f64>,
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
    pub exit: bool,
}

/// How many workspace cells the pager draws.
pub fn workspace_count(state: &ShellState) -> usize {
    state.workspaces.len().max(1)
}

// Fields needing `Option`/defaults for the manual Default impl.

impl ShellState {
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

    /// The bottom dock — a floating, centred icon strip (no exclusive
    /// zone: windows may slide underneath it, like the macOS Dock).
    pub fn create_dock(&mut self, qh: &QueueHandle<Self>) {
        let surface = self.compositor_state.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("cosmos-dock"),
            None,
        );
        layer.set_anchor(Anchor::BOTTOM);
        layer.set_size(dock::desired_width(self), dock::SURFACE_H);
        layer.set_exclusive_zone(0);
        layer.set_margin(0, 0, 8, 0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.wl_surface().commit();
        self.dock_surface = Some(layer);
        self.dock_dirty = true;
    }

    /// Resize the dock to its current item set (called when the window
    /// list or app set changes).
    pub fn sync_dock(&mut self) {
        if let Some(layer) = &self.dock_surface {
            let want = dock::desired_width(self);
            if want != self.dock_size.0 && want > 0 {
                layer.set_size(want, dock::SURFACE_H);
                layer.wl_surface().commit();
            } else {
                self.dock_dirty = true;
            }
        }
    }

    /// Toggle the quick-settings flyout (Win11-style tray popover).
    pub fn set_quick_open(&mut self, open: bool) {
        if open == self.quick_open {
            return;
        }
        self.quick_open = open;
        if open {
            self.vol_drag = false;
            let surface = self.compositor_state.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Top,
                Some("cosmos-quick"),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
            layer.set_size(quick::QUICK_W, quick::desired_height(&self.sysinfo));
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
        }
    }

    /// Click inside the quick-settings card.
    pub fn quick_click(&mut self, x: f64, y: f64) {
        self.quick_dirty = quick::press(self, x, y) || self.quick_dirty;
    }

    pub fn set_launcher_open(&mut self, open: bool) {
        if open == self.launcher_open {
            return;
        }
        self.launcher_open = open;
        if open {
            self.set_quick_open(false);
            self.launcher_query.clear();
            self.launcher_sel = 0;
            self.launcher_hover = None;
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
        }
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
    fn close_launcher(&mut self) {
        if self.launcher_open {
            self.set_launcher_open(false);
            self.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
        }
    }

    /// Pointer click inside the launcher surface.
    pub fn launcher_click(&mut self, x: f64, y: f64) {
        let hit = launcher::hit_test(
            x,
            y,
            self.launcher_size,
            self.filtered_apps().len(),
            launcher::pinned(self).len(),
            launcher::recommended(self).len(),
            !self.launcher_query.is_empty(),
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
            launcher::Hit::Action(_) | launcher::Hit::Input | launcher::Hit::List => {}
            launcher::Hit::Backdrop => self.close_launcher(),
        }
    }

    pub fn launch_selected(&mut self) {
        let apps = self.filtered_apps();
        let idx = self.launcher_sel.min(apps.len().saturating_sub(1));
        let Some(app) = apps.get(idx) else {
            return;
        };
        let app = (*app).clone();
        if let Err(err) = desktop::launch(&app) {
            tracing::warn!("launch {} failed: {err}", app.id);
        } else {
            self.record_launch(&app.id);
            self.close_launcher();
        }
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
            LauncherToggled { open } => self.set_launcher_open(open),
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
            Pong { .. } | Error { .. } => {}
        }
    }

    /// The tray's quick-settings click region — rightmost status text block.
    pub fn tray_clicked(&mut self, x: f64) -> bool {
        let (w, _) = self.panel_size;
        let status_w = crate::panel::status_text_len(&self.sysinfo);
        x >= w as f64 - status_w - 12.0
    }

    fn apply_config(&mut self, map: serde_json::Map<String, serde_json::Value>) {
        if let Some(v) = map.get("appearance").and_then(|v| v.as_str()) {
            self.dark = v == "dark";
            self.panel_dirty = true;
            self.launcher_dirty = true;
            self.notify_dirty = true;
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
        let before = self.notifications.len();
        let now = std::time::Instant::now();
        self.notifications.retain(|n| {
            let timeout = if n.timeout_ms <= 0 {
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
        if self.notifications.is_empty() && self.notify_surface.is_some() {
            self.notify_surface = None;
            return;
        }
        if !self.notifications.is_empty() && self.notify_surface.is_none() {
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
        if self.switcher_dirty && self.switcher_surface.is_some() {
            self.switcher_dirty = false;
            switcher::draw(self);
        }
        if self.assist_dirty && self.assist_surface.is_some() {
            self.assist_dirty = false;
            assist::draw(self);
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
            self.dock_size = (
                configure.new_size.0.max(dock::desired_width(self)),
                dock::SURFACE_H,
            );
            self.dock_dirty = true;
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
            self.quick_size = (quick::QUICK_W, quick::desired_height(&self.sysinfo));
            self.quick_dirty = true;
        }
        if self.assist_surface.as_ref() == Some(layer) {
            self.assist_size = configure.new_size;
            self.assist_dirty = true;
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
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if self.assist_open {
            assist::key_press(self, event);
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
            let Some(layer) = self
                .panel
                .iter()
                .chain(self.dock_surface.iter())
                .chain(self.launcher_surface.iter())
                .chain(self.assist_surface.iter())
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
                        self.dock_dirty = dock::click(self, ev.position.0);
                    } else if self.launcher_surface.as_ref() == Some(&layer) {
                        self.launcher_click(ev.position.0, ev.position.1);
                    } else if self.notify_surface.as_ref() == Some(&layer) {
                        popups::click(self, ev.position.1);
                    } else if self.quick_surface.as_ref() == Some(&layer) {
                        self.quick_click(ev.position.0, ev.position.1);
                    } else if self.assist_surface.as_ref() == Some(&layer) {
                        self.assist_click(ev.position.0, ev.position.1);
                    }
                }
                PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                    if self.panel.as_ref() == Some(&layer) {
                        self.panel_dirty |= panel::hover(self, ev.position.0, ev.position.1);
                    } else if self.dock_surface.as_ref() == Some(&layer) {
                        self.dock_dirty |= dock::hover(self, ev.position.0);
                    } else if self.launcher_surface.as_ref() == Some(&layer) {
                        self.launcher_dirty |= launcher::hover(self, ev.position.0, ev.position.1);
                    } else if self.quick_surface.as_ref() == Some(&layer) && self.vol_drag {
                        self.quick_dirty |= quick::drag(self, ev.position.0, ev.position.1);
                    } else if self.assist_surface.as_ref() == Some(&layer) {
                        self.assist_dirty |= assist::hover(self, ev.position.0, ev.position.1);
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
                    }
                }
                _ => {}
            }
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
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
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

impl ProvidesRegistryState for ShellState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers!(OutputState, SeatState);
}

smithay_client_toolkit::delegate_dispatch2!(ShellState);
smithay_client_toolkit::delegate_registry!(ShellState);

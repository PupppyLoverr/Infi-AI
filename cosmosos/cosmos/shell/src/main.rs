//! cosmos-shell — the Cosmos desktop shell: top panel, app launcher,
//! notification popups. Runs as a wlr-layer-shell Wayland client and talks
//! to cosmos-compositor over cosmos-ipc.

mod desktop;
mod draw;
mod ipc_client;
mod launcher;
mod notify;
mod panel;
mod popups;
mod quick;
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
        launcher_surface: None,
        notify_surface: None,
        quick_surface: None,
        notify_conn: None,
        panel_size: (0, PANEL_HEIGHT),
        launcher_size: (0, 0),
        notify_size: (0, 0),
        quick_size: (0, 0),
        windows: Vec::new(),
        workspaces: Vec::new(),
        launcher_open: false,
        launcher_query: String::new(),
        launcher_sel: 0,
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
        quick_open: false,
        vol_drag: false,
        exit: false,
    };

    state.create_panel(&qh);
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
    pub launcher_surface: Option<LayerSurface>,
    pub notify_surface: Option<LayerSurface>,
    pub quick_surface: Option<LayerSurface>,
    pub notify_conn: Option<zbus::blocking::Connection>,
    pub panel_size: (u32, u32),
    pub launcher_size: (u32, u32),
    pub notify_size: (u32, u32),
    pub quick_size: (u32, u32),

    pub windows: Vec<cosmos_ipc::WindowInfo>,
    pub workspaces: Vec<cosmos_ipc::WorkspaceInfo>,
    pub launcher_open: bool,
    pub launcher_query: String,
    pub launcher_sel: usize,
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
    /// Quick-settings flyout state.
    pub quick_open: bool,
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
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
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

    /// Pointer click inside the launcher surface.
    pub fn launcher_click(&mut self, x: f64, y: f64) {
        let hit = launcher::hit_test(x, y, self.launcher_size, self.filtered_apps().len());
        match hit {
            launcher::Hit::Item(idx) => {
                self.launcher_sel = idx;
                self.launch_selected();
            }
            launcher::Hit::Action(0) => {
                let _ = std::process::Command::new("cosmos-settings").spawn();
                self.set_launcher_open(false);
            }
            launcher::Hit::Action(1) => {
                if let Some(app) = self.apps.iter().find(|a| a.id == "sys.logout").cloned() {
                    let _ = desktop::launch(&app);
                }
                self.set_launcher_open(false);
            }
            launcher::Hit::Action(_) | launcher::Hit::Input | launcher::Hit::List => {}
            launcher::Hit::Backdrop => self.set_launcher_open(false),
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
            self.set_launcher_open(false);
        }
    }

    fn on_ipc_event(&mut self, ev: cosmos_ipc::Event) {
        use cosmos_ipc::Event::*;
        match ev {
            Windows { windows } => {
                self.windows = windows;
                self.panel_dirty = true;
            }
            Workspaces { workspaces } => {
                self.workspaces = workspaces;
                self.panel_dirty = true;
            }
            LauncherToggled { open } => self.set_launcher_open(open),
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
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if self.launcher_open {
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
                .chain(self.launcher_surface.iter())
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
                    } else if self.launcher_surface.as_ref() == Some(&layer) {
                        self.launcher_click(ev.position.0, ev.position.1);
                    } else if self.notify_surface.as_ref() == Some(&layer) {
                        popups::click(self, ev.position.1);
                    } else if self.quick_surface.as_ref() == Some(&layer) {
                        self.quick_click(ev.position.0, ev.position.1);
                    }
                }
                PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                    if self.panel.as_ref() == Some(&layer) {
                        self.panel_dirty |= panel::hover(self, ev.position.0, ev.position.1);
                    } else if self.launcher_surface.as_ref() == Some(&layer) {
                        self.launcher_dirty |= launcher::hover(self, ev.position.0, ev.position.1);
                    } else if self.quick_surface.as_ref() == Some(&layer) && self.vol_drag {
                        self.quick_dirty |= quick::drag(self, ev.position.0, ev.position.1);
                    }
                }
                PointerEventKind::Release { .. } => {
                    if self.quick_surface.as_ref() == Some(&layer) {
                        quick::release(self);
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

//! Cosmos desktop state: workspaces, minimize, snapping, window registry,
//! persisted config, and IPC broadcast plumbing.
//!
//! `self.space` in `AnvilState` is always the *active* workspace. Windows on
//! other workspaces are parked in [`CosmosState::offscreen`]; minimized windows
//! live in [`CosmosState::minimized`]. This keeps smithay's `Space` semantics
//! untouched while giving us real virtual desktops.

use std::{
    collections::HashMap,
    io::Write,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use smithay::{
    desktop::{layer_map_for_output, space::SpaceElement},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{protocol::wl_surface::WlSurface, Resource},
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};
use tracing::{info, warn};

use crate::{
    focus::KeyboardFocusTarget,
    shell::WindowElement,
    state::{AnvilState, Backend},
};

/// Number of virtual desktops. Exposed as workspaces 1..9.
pub const WORKSPACE_COUNT: usize = 9;

/// Horizontal/vertical padding around tiled windows.
const SNAP_GAP: i32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CosmosConfig {
    /// "dark" or "light"
    pub appearance: String,
    /// Logical output scale override (1.0 = native).
    pub scale: f64,
    /// Disable window/workspace transition animation.
    pub reduce_motion: bool,
}

impl Default for CosmosConfig {
    fn default() -> Self {
        Self {
            appearance: "dark".to_string(),
            scale: 1.0,
            reduce_motion: false,
        }
    }
}

impl CosmosConfig {
    fn path() -> PathBuf {
        if let Ok(p) = std::env::var("COSMOS_CONFIG") {
            return PathBuf::from(p);
        }
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(".config"))
                    .unwrap_or_else(|_| PathBuf::from("/tmp"))
            });
        base.join("cosmos").join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(cfg) => {
                    info!(?path, "loaded cosmos config");
                    cfg
                }
                Err(err) => {
                    warn!(?path, "bad cosmos config ({err}); using defaults");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(err) = std::fs::write(&path, text) {
                    warn!(?path, "failed to persist cosmos config: {err}");
                }
            }
            Err(err) => warn!("failed to serialize cosmos config: {err}"),
        }
    }

    /// Serialize to a JSON map for IPC `Config` events.
    pub fn as_map(&self) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        }
    }

    pub fn apply_patch(&mut self, key: &str, value: serde_json::Value) -> Result<(), String> {
        match key {
            "appearance" => {
                let v = value
                    .as_str()
                    .ok_or_else(|| "appearance must be a string".to_string())?;
                if v != "dark" && v != "light" {
                    return Err("appearance must be \"dark\" or \"light\"".to_string());
                }
                self.appearance = v.to_string();
            }
            "scale" => {
                let v = value
                    .as_f64()
                    .ok_or_else(|| "scale must be a number".to_string())?;
                if !(0.5..=3.0).contains(&v) {
                    return Err("scale out of range 0.5..=3.0".to_string());
                }
                self.scale = v;
            }
            "reduce_motion" => {
                self.reduce_motion = value
                    .as_bool()
                    .ok_or_else(|| "reduce_motion must be a bool".to_string())?;
            }
            other => return Err(format!("unknown config key: {other}")),
        }
        Ok(())
    }
}

/// Theme derived from config — the Cosmos monochrome language.
#[derive(Debug, Clone, Copy)]
pub struct CosmosTheme {
    pub dark: bool,
    /// Desktop background.
    pub background: [f32; 4],
    /// Titlebar background, focused window.
    pub titlebar_bg_focused: [f32; 4],
    /// Titlebar background, unfocused window.
    pub titlebar_bg: [f32; 4],
    /// Titlebar text / button glyph color.
    pub titlebar_fg: [f32; 4],
    /// Titlebar button hover fill.
    pub button_hover: [f32; 4],
    /// Focused-window outline tint.
    pub focus_ring: [f32; 4],
}

impl CosmosTheme {
    pub fn from_config(cfg: &CosmosConfig) -> Self {
        if cfg.appearance == "light" {
            Self {
                dark: false,
                background: [0.89, 0.90, 0.92, 1.0],
                titlebar_bg_focused: [0.97, 0.97, 0.98, 1.0],
                titlebar_bg: [0.90, 0.91, 0.92, 1.0],
                titlebar_fg: [0.10, 0.10, 0.12, 1.0],
                button_hover: [0.82, 0.83, 0.85, 1.0],
                focus_ring: [0.60, 0.62, 0.66, 1.0],
            }
        } else {
            Self {
                dark: true,
                background: [0.07, 0.075, 0.08, 1.0],
                titlebar_bg_focused: [0.13, 0.135, 0.15, 1.0],
                titlebar_bg: [0.10, 0.105, 0.115, 1.0],
                titlebar_fg: [0.91, 0.91, 0.92, 1.0],
                button_hover: [0.26, 0.27, 0.30, 1.0],
                focus_ring: [0.38, 0.40, 0.45, 1.0],
            }
        }
    }
}

static WINDOW_IDS: AtomicU64 = AtomicU64::new(1);

/// A client of the compositor IPC socket.
#[derive(Debug)]
pub struct IpcClient {
    /// Stable id — the client list compacts on drop, so positions shift.
    pub id: u64,
    pub stream: UnixStream,
    pub subscribed: bool,
    pub dead: bool,
    /// Partial-line accumulator for the NDJSON protocol.
    pub read_buf: Vec<u8>,
    /// Serialized-but-not-yet-written events. The socket is nonblocking;
    /// EAGAIN queues here and drains on the next dispatch/flush instead of
    /// killing the client.
    pub outbox: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct IpcState {
    pub clients: Vec<IpcClient>,
}

/// Which snap/tiling a window is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapState {
    Floating,
    Left,
    Right,
    Maximized,
}

/// The modifier that opened the window switcher; committing happens when
/// it is released, matching macOS/Windows Alt+Tab and Cmd/Super+Tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldMod {
    Alt,
    Logo,
}

/// Open Alt/Super+Tab switcher: stable window order + highlighted index.
#[derive(Debug)]
pub struct SwitcherState {
    pub order: Vec<WindowElement>,
    pub sel: usize,
    pub held: HeldMod,
}

/// Snap Assist (Win11): after a Left/Right snap the picker offers the
/// remaining windows to fill the free half. `fill` is the SnapState the
/// picked window receives; `candidates` are eligible window ids.
#[derive(Debug)]
pub struct AssistState {
    pub fill: SnapState,
    pub candidates: Vec<u64>,
}

/// Cosmos-managed window state, keyed by compositor window id.
#[derive(Debug)]
pub struct WindowMeta {
    pub id: u64,
    /// The surface identity key (`wl_surface` object id, stringified).
    pub surface_key: String,
    /// Workspace the window belongs to (0-based).
    pub workspace: usize,
    pub minimized: bool,
    /// Floating geometry to restore to (un-snap/un-minimize).
    pub restore: Option<Rectangle<i32, Logical>>,
    pub snap: SnapState,
    /// Position to re-map at after an unmap/remap round trip.
    pub parked_loc: Point<i32, Logical>,
}

#[derive(Debug, Default)]
pub struct CosmosState {
    /// Active virtual desktop, 0-based.
    pub active_workspace: usize,
    /// Windows parked on non-active workspaces: ws → windows.
    /// Position inside each window's meta (`parked_loc`).
    pub parked: HashMap<usize, Vec<WindowElement>>,
    /// window id → metadata.
    pub windows: HashMap<u64, WindowMeta>,
    /// surface_key → window id.
    pub ids: HashMap<String, u64>,
    /// IPC socket clients + path.
    pub ipc: IpcState,
    pub ipc_socket_path: Option<PathBuf>,
    pub config: CosmosConfig,
    /// Currently-open layer-shell launcher hint (panel handles visuals).
    pub launcher_open: bool,
    /// Alt/Super+Tab switcher while the modifier is held.
    pub switcher: Option<SwitcherState>,
    /// Live drag-to-edge snap hint: the zone the pointer targets and the
    /// exact rectangle the window would snap into on release (Win11's
    /// translucent drop preview).
    pub snap_preview: Option<(SnapState, Rectangle<i32, Logical>)>,
    /// Snap Assist picker while it is offered (shell renders visuals).
    pub assist: Option<AssistState>,
    /// If set, broadcast a `Windows`+`Workspaces` update at the next idle point.
    pub dirty: bool,
}

impl CosmosState {
    pub fn theme(&self) -> CosmosTheme {
        CosmosTheme::from_config(&self.config)
    }

    /// Stable id for a window, minting one if needed.
    pub fn window_id(&mut self, window: &WindowElement) -> u64 {
        let key = surface_key(window);
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        let id = WINDOW_IDS.fetch_add(1, Ordering::Relaxed);
        self.ids.insert(key.clone(), id);
        self.windows.insert(
            id,
            WindowMeta {
                id,
                surface_key: key,
                workspace: self.active_workspace,
                minimized: false,
                restore: None,
                snap: SnapState::Floating,
                parked_loc: (0, 0).into(),
            },
        );
        id
    }

    /// Look up a window's id without minting.
    pub fn id_of(&self, window: &WindowElement) -> Option<u64> {
        self.ids.get(&surface_key(window)).copied()
    }

    pub fn meta(&self, id: u64) -> Option<&WindowMeta> {
        self.windows.get(&id)
    }

    pub fn meta_mut(&mut self, id: u64) -> Option<&mut WindowMeta> {
        self.windows.get_mut(&id)
    }

    pub fn meta_of(&self, window: &WindowElement) -> Option<&WindowMeta> {
        self.id_of(window).and_then(|id| self.windows.get(&id))
    }
}

/// Identity key for a window: owning client id + root `WlSurface` object id.
/// The object id alone is client-local — two clients both get `wl_surface@N`
/// — so the client's backend id must disambiguate or every window would
/// dedupe to the same registry entry.
pub fn surface_key(window: &WindowElement) -> String {
    window
        .wl_surface()
        .map(|s| {
            let cid = s
                .client()
                .map(|c| format!("{:?}", c.id()))
                .unwrap_or_else(|| "none".to_string());
            format!("{cid}:{}", s.id())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Read the client-set title + app_id of a toplevel surface.
pub fn toplevel_title_app(wl_surface: &WlSurface) -> (Option<String>, Option<String>) {
    with_states(wl_surface, |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .map(|data| {
                let attrs = data.lock().unwrap();
                (attrs.title.clone(), attrs.app_id.clone())
            })
            .unwrap_or_default()
    })
}

impl<BackendData: Backend> AnvilState<BackendData> {
    /// The workspace index a window is assigned to.
    pub fn window_workspace(&self, window: &WindowElement) -> usize {
        self.cosmos
            .meta_of(window)
            .map(|m| m.workspace)
            .unwrap_or(self.cosmos.active_workspace)
    }

    /// Window lookup across active space + parked + minimized.
    pub fn window_by_id(&self, id: u64) -> Option<WindowElement> {
        if let Some(w) = self
            .space
            .elements()
            .find(|w| self.cosmos.id_of(w) == Some(id))
        {
            return Some(w.clone());
        }
        self.cosmos
            .parked
            .values()
            .flat_map(|v| v.iter())
            .find(|w| self.cosmos.id_of(w) == Some(id))
            .cloned()
    }

    /// Every managed window (active + parked + minimized).
    pub fn all_windows(&self) -> Vec<WindowElement> {
        let mut all: Vec<WindowElement> = self.space.elements().cloned().collect();
        for v in self.cosmos.parked.values() {
            all.extend(v.iter().cloned());
        }
        all
    }

    /// Park all current-space windows on `ws` (used by workspace switch).
    fn park_current(&mut self, ws: usize) {
        let elements: Vec<WindowElement> = self.space.elements().cloned().collect();
        let mut to_push = Vec::with_capacity(elements.len());
        for w in elements {
            let loc = self.space.element_location(&w).unwrap_or_default();
            let id = self.cosmos.window_id(&w);
            if let Some(meta) = self.cosmos.windows.get_mut(&id) {
                meta.workspace = ws;
                meta.parked_loc = loc;
            }
            // Deactivate before parking so a re-map can't resurrect a
            // stale pending Activated alongside the newly focused window.
            w.0.set_activated(false);
            self.space.unmap_elem(&w);
            to_push.push(w);
        }
        self.cosmos.parked.entry(ws).or_default().extend(to_push);
    }

    /// Restore parked windows of `ws` into the space.
    fn unpark(&mut self, ws: usize) {
        let Some(list) = self.cosmos.parked.remove(&ws) else {
            return;
        };
        for w in list {
            let loc = self
                .cosmos
                .meta_of(&w)
                .map(|m| m.parked_loc)
                .unwrap_or_default();
            let id = self.cosmos.window_id(&w);
            if let Some(meta) = self.cosmos.windows.get_mut(&id) {
                meta.workspace = ws;
            }
            self.space.map_element(w, loc, false);
        }
    }

    /// Switch the active virtual desktop.
    pub fn switch_workspace(&mut self, target: usize) {
        if target >= WORKSPACE_COUNT || target == self.cosmos.active_workspace {
            return;
        }
        let from = self.cosmos.active_workspace;
        self.close_snap_assist();
        self.park_current(from);
        self.cosmos.active_workspace = target;
        self.unpark(target);

        // Focus + activate the top window on the new workspace, if any.
        // Parked windows keep their pending Activated state otherwise, and
        // keyboard focus without activation desyncs the SSD focus discs.
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().unwrap();
        let top = self.space.elements().last().cloned();
        if let Some(top) = &top {
            self.space.raise_element(top, true);
        }
        keyboard.set_focus(self, top.map(KeyboardFocusTarget::from), serial);
        self.flush_pending_configures();
        self.cosmos.dirty = true;
    }

    /// Move a window to another workspace (keeping it mapped there).
    pub fn move_window_to_workspace(&mut self, window: &WindowElement, ws: usize) {
        if ws >= WORKSPACE_COUNT {
            return;
        }
        let id = self.cosmos.window_id(window);
        if ws == self.cosmos.active_workspace {
            if let Some(meta) = self.cosmos.windows.get_mut(&id) {
                meta.workspace = ws;
            }
            self.cosmos.dirty = true;
            return;
        }
        let loc = self
            .space
            .element_location(window)
            .unwrap_or_else(|| window.geometry().loc);
        self.space.unmap_elem(window);
        if let Some(meta) = self.cosmos.windows.get_mut(&id) {
            meta.workspace = ws;
            meta.parked_loc = loc;
        }
        self.cosmos
            .parked
            .entry(ws)
            .or_default()
            .push(window.clone());
        self.cosmos.dirty = true;
    }

    /// Minimize a window to the task area.
    pub fn minimize_window(&mut self, window: &WindowElement) {
        let id = self.cosmos.window_id(window);
        let loc = self.space.element_location(window).unwrap_or_default();
        window.0.set_activated(false);
        self.space.unmap_elem(window);
        if let Some(meta) = self.cosmos.windows.get_mut(&id) {
            meta.minimized = true;
            meta.parked_loc = loc;
        }
        // Park on the "minimized" lane, indexed at WORKSPACE_COUNT.
        self.cosmos
            .parked
            .entry(WORKSPACE_COUNT)
            .or_default()
            .push(window.clone());
        // Move focus + activation to whatever is next.
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().unwrap();
        let top = self.space.elements().last().cloned();
        if let Some(top) = &top {
            self.space.raise_element(top, true);
        }
        keyboard.set_focus(self, top.map(KeyboardFocusTarget::from), serial);
        self.flush_pending_configures();
        self.cosmos.dirty = true;
    }

    /// Restore a minimized window.
    pub fn unminimize_window(&mut self, id: u64) {
        let Some(key) = self.cosmos.windows.get(&id).map(|m| m.surface_key.clone()) else {
            return;
        };
        let Some(list) = self.cosmos.parked.get_mut(&WORKSPACE_COUNT) else {
            return;
        };
        let Some(pos) = list.iter().position(|w| surface_key(w) == key) else {
            return;
        };
        let entry = list.remove(pos);
        // Restore to the window's own workspace.
        let ws = self
            .cosmos
            .windows
            .get(&id)
            .map(|m| m.workspace)
            .unwrap_or(self.cosmos.active_workspace);
        if let Some(meta) = self.cosmos.windows.get_mut(&id) {
            meta.minimized = false;
        }
        if ws == self.cosmos.active_workspace {
            let loc = self
                .cosmos
                .windows
                .get(&id)
                .map(|m| m.parked_loc)
                .unwrap_or_default();
            self.space.map_element(entry.clone(), loc, true);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let keyboard = self.seat.get_keyboard().unwrap();
            keyboard.set_focus(self, Some(KeyboardFocusTarget::from(entry)), serial);
            self.flush_pending_configures();
        } else {
            self.cosmos.parked.entry(ws).or_default().push(entry);
        }
        self.cosmos.dirty = true;
    }

    /// The work area of the output under the pointer (excludes panel layer shells).
    pub fn work_area(&self, window: Option<&WindowElement>) -> Rectangle<i32, Logical> {
        let output = window
            .and_then(|w| self.space.outputs_for_element(w).first().cloned())
            .or_else(|| {
                self.space
                    .output_under(self.pointer.current_location())
                    .next()
                    .cloned()
            })
            .or_else(|| self.space.outputs().next().cloned());
        output
            .and_then(|o| {
                let geo = self.space.output_geometry(&o)?;
                let map = layer_map_for_output(&o);
                let zone = map.non_exclusive_zone();
                Some(Rectangle::new(geo.loc + zone.loc, zone.size))
            })
            .unwrap_or_else(|| Rectangle::from_size((800, 600).into()))
    }

    /// Rectangle a snap of `window` to `snap` would occupy — the same
    /// loc/size `snap_window` maps the window to (including the SSD
    /// titlebar band, so the preview covers the full target frame).
    /// Used to render the drag-to-edge drop preview.
    pub fn snap_target_rect(
        &self,
        window: &WindowElement,
        snap: SnapState,
    ) -> Option<Rectangle<i32, Logical>> {
        let area = self.work_area(Some(window));
        let (loc, size) = match snap {
            SnapState::Left => (
                area.loc + Point::from((SNAP_GAP, SNAP_GAP)),
                Size::from((area.size.w / 2 - SNAP_GAP * 2, area.size.h - SNAP_GAP * 2)),
            ),
            SnapState::Right => (
                area.loc + Point::from((area.size.w / 2 + SNAP_GAP, SNAP_GAP)),
                Size::from((area.size.w / 2 - SNAP_GAP * 2, area.size.h - SNAP_GAP * 2)),
            ),
            SnapState::Maximized => (area.loc, area.size),
            SnapState::Floating => return None,
        };
        Some(Rectangle::new(loc, size))
    }

    /// Which edge-snap zone the pointer sits in while dragging a window:
    /// 12px bands along the work area's edges — Win11 idiom: left/right
    /// = half-tile, top = maximize. `None` outside the zone edges.
    ///
    /// The bands follow the pointer wherever it travels, including over
    /// the exclusive menubar — a titlebar drag to the screen's top-left
    /// corner otherwise spends its whole approach in `y < work_area.y`,
    /// and the old `loc.y < ay` early-return made the zones unreachable.
    pub fn edge_snap_zone(
        &self,
        loc: Point<f64, Logical>,
        window: &WindowElement,
    ) -> Option<SnapState> {
        let area = self.work_area(Some(window));
        const EDGE: f64 = 12.0;
        let (ax, ay, aw) = (area.loc.x as f64, area.loc.y as f64, area.size.w as f64);
        if loc.x <= ax + EDGE {
            return Some(SnapState::Left);
        }
        if loc.x >= ax + aw - EDGE {
            return Some(SnapState::Right);
        }
        if loc.y <= ay + EDGE {
            return Some(SnapState::Maximized);
        }
        None
    }

    /// Snap the focused window left/right/maximize or restore.
    pub fn snap_window(&mut self, window: &WindowElement, snap: SnapState) {
        self.snap_window_with_assist(window, snap, true)
    }

    fn snap_window_with_assist(
        &mut self,
        window: &WindowElement,
        snap: SnapState,
        offer_assist: bool,
    ) {
        let Some(surface) = window.0.toplevel() else {
            return;
        };
        let id = self.cosmos.window_id(window);
        let current_loc = self.space.element_location(window).unwrap_or_default();
        // `window.geometry()` includes the SSD titlebar; surface sizes
        // must exclude it or every snap/restore cycle grows the window
        // by 32px and overflows the zone.
        let titlebar = window.titlebar_height();
        let geo = window.geometry().size;
        let current_size = Size::from((geo.w, geo.h - titlebar));

        // Save floating geometry when leaving Floating for the first time.
        {
            let meta = self.cosmos.windows.get_mut(&id).unwrap();
            if meta.snap == SnapState::Floating && snap != SnapState::Floating {
                meta.restore = Some(Rectangle::new(current_loc, current_size));
            }
            meta.snap = snap;
        }

        let area = self.work_area(Some(window));
        let (loc, size, state) = match snap {
            SnapState::Left => (
                area.loc + Point::from((SNAP_GAP, SNAP_GAP)),
                Size::from((
                    area.size.w / 2 - SNAP_GAP * 2,
                    area.size.h - SNAP_GAP * 2 - titlebar,
                )),
                xdg_toplevel::State::TiledLeft,
            ),
            SnapState::Right => (
                area.loc + Point::from((area.size.w / 2 + SNAP_GAP, SNAP_GAP)),
                Size::from((
                    area.size.w / 2 - SNAP_GAP * 2,
                    area.size.h - SNAP_GAP * 2 - titlebar,
                )),
                xdg_toplevel::State::TiledRight,
            ),
            SnapState::Maximized => (
                area.loc,
                Size::from((area.size.w, area.size.h - titlebar)),
                xdg_toplevel::State::Maximized,
            ),
            SnapState::Floating => {
                let restore = self
                    .cosmos
                    .windows
                    .get(&id)
                    .and_then(|m| m.restore)
                    .unwrap_or(Rectangle::new(current_loc, current_size));
                surface.with_pending_state(|s| {
                    s.states.unset(xdg_toplevel::State::TiledLeft);
                    s.states.unset(xdg_toplevel::State::TiledRight);
                    s.states.unset(xdg_toplevel::State::Maximized);
                    s.size = Some(restore.size);
                });
                surface.send_pending_configure();
                self.space.map_element(window.clone(), restore.loc, false);
                if let Some(fit) = window.user_data().get::<crate::shell::InitialFit>() {
                    fit.user_moved();
                }
                self.flush_pending_configures();
                self.close_snap_assist();
                self.cosmos.dirty = true;
                return;
            }
        };

        surface.with_pending_state(|s| {
            s.states.unset(xdg_toplevel::State::TiledLeft);
            s.states.unset(xdg_toplevel::State::TiledRight);
            s.states.unset(xdg_toplevel::State::Maximized);
            s.states.set(state);
            s.size = Some(size);
        });
        surface.send_pending_configure();
        self.space.map_element(window.clone(), loc, true);
        if let Some(fit) = window.user_data().get::<crate::shell::InitialFit>() {
            fit.user_moved();
        }
        self.flush_pending_configures();
        match (offer_assist, snap) {
            (true, SnapState::Left | SnapState::Right) => self.offer_snap_assist(snap, id),
            (true, _) => self.close_snap_assist(),
            (false, _) => {}
        }
        self.cosmos.dirty = true;
    }

    /// Open the Snap Assist picker: offer the other non-minimized
    /// windows on this workspace to fill the half `snapped` left free.
    fn offer_snap_assist(&mut self, snapped: SnapState, snapped_id: u64) {
        let fill = match snapped {
            SnapState::Left => SnapState::Right,
            _ => SnapState::Left,
        };
        let ws = self.cosmos.active_workspace;
        let candidates: Vec<u64> = self
            .all_windows()
            .iter()
            .filter_map(|w| {
                let id = self.cosmos.id_of(w)?;
                let m = self.cosmos.meta(id)?;
                (id != snapped_id && m.workspace == ws && !m.minimized).then_some(id)
            })
            .collect();
        if candidates.is_empty() {
            self.close_snap_assist();
            return;
        }
        self.cosmos.assist = Some(AssistState {
            fill,
            candidates: candidates.clone(),
        });
        self.ipc_broadcast(&cosmos_ipc::Event::SnapAssist {
            open: true,
            fill: if fill == SnapState::Left {
                "left".to_string()
            } else {
                "right".to_string()
            },
            candidates,
        });
    }

    /// Close Snap Assist if it is open (idempotent).
    pub fn close_snap_assist(&mut self) {
        if self.cosmos.assist.take().is_some() {
            self.ipc_broadcast(&cosmos_ipc::Event::SnapAssist {
                open: false,
                fill: String::new(),
                candidates: Vec::new(),
            });
        }
    }

    /// Snap Assist pick: snap `id` into the free half and focus it.
    pub fn snap_assist_pick(&mut self, id: u64) {
        let Some(assist) = self.cosmos.assist.take() else {
            return;
        };
        self.ipc_broadcast(&cosmos_ipc::Event::SnapAssist {
            open: false,
            fill: String::new(),
            candidates: Vec::new(),
        });
        if !assist.candidates.contains(&id) {
            return;
        }
        let Some(window) = self.window_by_id(id) else {
            return;
        };
        self.snap_window_with_assist(&window, assist.fill, false);
        self.focus_window(&window);
    }

    /// Snap Assist dismissed without a pick.
    pub fn snap_assist_dismiss(&mut self) {
        self.close_snap_assist();
    }

    /// Push every pending toplevel configure (activation changes, bounds)
    /// so clients ack them — the SSD focus discs read the *acked* state,
    /// and unfocused clients otherwise wait indefinitely for the next
    /// configure to repaint.
    pub fn flush_pending_configures(&mut self) {
        for window in self.space.elements() {
            if let Some(toplevel) = window.0.toplevel() {
                toplevel.send_pending_configure();
            }
        }
    }

    /// Focus + raise a window, un-minimizing and switching workspace as needed.
    pub fn focus_window(&mut self, window: &WindowElement) {
        let id = self.cosmos.window_id(window);
        let meta_ws = self.cosmos.windows.get(&id).map(|m| m.workspace);
        let minimized = self
            .cosmos
            .windows
            .get(&id)
            .map(|m| m.minimized)
            .unwrap_or(false);
        if minimized {
            self.unminimize_window(id);
        } else if let Some(ws) = meta_ws {
            if ws != self.cosmos.active_workspace {
                self.switch_workspace(ws);
            }
        }
        if self.space.elements().any(|w| w == window) {
            self.space.raise_element(window, true);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let keyboard = self.seat.get_keyboard().unwrap();
            keyboard.set_focus(
                self,
                Some(KeyboardFocusTarget::from(window.clone())),
                serial,
            );
            self.flush_pending_configures();
        }
        self.cosmos.dirty = true;
    }

    /// Alt+Tab / Super+Tab step: open the switcher on the first press
    /// (selecting the window after the focused one), then advance the
    /// highlight per press. Focus only changes on [`Self::switcher_commit`],
    /// when the modifier that opened it is released.
    pub fn switcher_step(&mut self, backwards: bool) {
        // Forget windows that went away since the switcher opened.
        let live: Vec<WindowElement> = self.space.elements().cloned().collect();
        if let Some(sw) = &mut self.cosmos.switcher {
            sw.order.retain(|w| live.contains(w));
            if sw.order.is_empty() {
                self.cosmos.switcher = None;
            } else {
                sw.sel = sw.sel.min(sw.order.len() - 1);
            }
        }
        if self.cosmos.switcher.is_none() {
            let order: Vec<WindowElement> = self.space.elements().cloned().collect();
            if order.is_empty() {
                return;
            }
            let held = if self
                .seat
                .get_keyboard()
                .map(|k| k.modifier_state().alt)
                .unwrap_or(false)
            {
                HeldMod::Alt
            } else {
                HeldMod::Logo
            };
            let focused = self.focused_window();
            let sel = match focused.and_then(|f| order.iter().position(|w| w == &f)) {
                Some(i) if order.len() > 1 => {
                    if backwards {
                        (i + order.len() - 1) % order.len()
                    } else {
                        (i + 1) % order.len()
                    }
                }
                _ => 0,
            };
            self.cosmos.switcher = Some(SwitcherState { order, sel, held });
        } else if let Some(sw) = &mut self.cosmos.switcher {
            let n = sw.order.len();
            sw.sel = if backwards {
                (sw.sel + n - 1) % n
            } else {
                (sw.sel + 1) % n
            };
        }
        let Some(sw) = &self.cosmos.switcher else {
            return;
        };
        let order: Vec<u64> = sw
            .order
            .iter()
            .map(|w| self.cosmos.id_of(w).unwrap_or(0))
            .collect();
        let selected = order.get(sw.sel).copied().unwrap_or(0);
        self.ipc_broadcast(&cosmos_ipc::Event::Switcher {
            open: true,
            selected,
            order,
        });
    }

    /// Commit the highlighted switcher window: focus it and close the
    /// switcher. Called when the held modifier is released.
    pub fn switcher_commit(&mut self) {
        let Some(sw) = self.cosmos.switcher.take() else {
            return;
        };
        if let Some(window) = sw.order.get(sw.sel).cloned() {
            if self.all_windows().contains(&window) {
                self.focus_window(&window);
            }
        }
        self.ipc_broadcast(&cosmos_ipc::Event::Switcher {
            open: false,
            selected: 0,
            order: Vec::new(),
        });
        self.cosmos.dirty = true;
    }

    /// The focused window, if any.
    pub fn focused_window(&self) -> Option<WindowElement> {
        self.seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .and_then(|target| match target {
                KeyboardFocusTarget::Window(w) => Some(WindowElement(w)),
                _ => None,
            })
    }

    /// Build the IPC `Windows` event payload.
    pub fn ipc_windows(&self) -> Vec<cosmos_ipc::WindowInfo> {
        self.all_windows()
            .into_iter()
            .filter_map(|w| self.ipc_window_info(&w))
            .collect()
    }

    fn ipc_window_info(&self, w: &WindowElement) -> Option<cosmos_ipc::WindowInfo> {
        let id = self.cosmos.id_of(w)?;
        let meta = self.cosmos.windows.get(&id);
        let toplevel = w.0.toplevel();
        let (title, app_id) = w
            .wl_surface()
            .as_deref()
            .map(|s| toplevel_title_app(s))
            .map(|(t, a)| (t.unwrap_or_default(), a.unwrap_or_default()))
            .unwrap_or_default();
        let loc = if self.space.elements().any(|e| e == w) {
            self.space.element_location(w).unwrap_or_default()
        } else {
            meta.map(|m| m.parked_loc).unwrap_or_default()
        };
        let geo = w.geometry();
        let focused = self.focused_window().map(|f| f == *w).unwrap_or(false);
        let states = toplevel
            .map(|t| t.current_state().states)
            .unwrap_or_default();
        let output = self
            .space
            .outputs_for_element(w)
            .first()
            .map(|o| o.name())
            .unwrap_or_default();
        Some(cosmos_ipc::WindowInfo {
            id,
            title,
            app_id,
            workspace: meta.map(|m| m.workspace).unwrap_or(0) as u8,
            x: loc.x,
            y: loc.y,
            w: geo.size.w,
            h: geo.size.h,
            focused,
            minimized: meta.map(|m| m.minimized).unwrap_or(false),
            maximized: states.contains(xdg_toplevel::State::Maximized),
            fullscreen: states.contains(xdg_toplevel::State::Fullscreen),
            output,
        })
    }

    /// IPC `Workspaces` event payload.
    pub fn ipc_workspaces(&self) -> Vec<cosmos_ipc::WorkspaceInfo> {
        let mut counts = vec![0usize; WORKSPACE_COUNT];
        for meta in self.cosmos.windows.values() {
            if meta.workspace < WORKSPACE_COUNT && !meta.minimized {
                counts[meta.workspace] += 1;
            }
        }
        (0..WORKSPACE_COUNT)
            .map(|i| cosmos_ipc::WorkspaceInfo {
                id: i as u8,
                focused: i == self.cosmos.active_workspace,
                window_count: counts[i],
            })
            .collect()
    }

    /// Broadcast current window + workspace state to IPC subscribers.
    pub fn ipc_broadcast_state(&mut self) {
        let windows = cosmos_ipc::Event::Windows {
            windows: self.ipc_windows(),
        };
        let workspaces = cosmos_ipc::Event::Workspaces {
            workspaces: self.ipc_workspaces(),
        };
        self.ipc_broadcast(&windows);
        self.ipc_broadcast(&workspaces);
    }

    /// Send an event to every connected IPC client.
    pub fn ipc_broadcast(&mut self, event: &cosmos_ipc::Event) {
        for client in &mut self.cosmos.ipc.clients {
            if client.dead {
                continue;
            }
            Self::ipc_push(client, event);
        }
        self.cosmos.ipc.clients.retain(|c| !c.dead);
    }

    /// Queue one event into a client's outbox and write what fits.
    /// Outlives transient EAGAIN instead of dropping the client.
    pub fn ipc_push(client: &mut IpcClient, event: &cosmos_ipc::Event) {
        if cosmos_ipc::write_message(&mut client.outbox, event).is_err() {
            return;
        }
        Self::ipc_flush_client(client);
    }

    /// Drain as much of a client's outbox as the socket accepts right now.
    /// Returns false (and marks the client dead) on a real error.
    pub fn ipc_flush_client(client: &mut IpcClient) {
        while !client.outbox.is_empty() {
            match client.stream.write(&client.outbox) {
                Ok(0) => break,
                Ok(n) => {
                    client.outbox.drain(..n);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) => {
                    tracing::debug!("ipc client write failed: {err}");
                    let _ = client.stream.shutdown(std::net::Shutdown::Both);
                    client.dead = true;
                    return;
                }
            }
        }
    }

    /// Respond to one client only.
    pub fn ipc_send(client: &mut UnixStream, event: &cosmos_ipc::Event) {
        let _ = cosmos_ipc::write_message(client, event);
    }

    /// Apply a config key patch, persist + broadcast.
    pub fn cosmos_set_config(&mut self, key: &str, value: serde_json::Value) -> Result<(), String> {
        self.cosmos.config.apply_patch(key, value)?;
        self.cosmos.config.save();
        crate::shell::ssd::set_global_theme(self.cosmos.theme());
        // All titlebars repaint next frame.
        for window in self.all_windows() {
            if let Some(state) = window
                .user_data()
                .get::<std::cell::RefCell<crate::shell::ssd::WindowState>>()
            {
                state.borrow_mut().header_bar.invalidate();
            }
        }
        let ev = cosmos_ipc::Event::Config(self.cosmos.config.as_map());
        self.ipc_broadcast(&ev);
        Ok(())
    }

    /// Flush queued state broadcasts and any pending client outboxes.
    pub fn ipc_flush(&mut self) {
        for client in &mut self.cosmos.ipc.clients {
            if !client.dead {
                Self::ipc_flush_client(client);
            }
        }
        self.cosmos.ipc.clients.retain(|c| !c.dead);
        if self.cosmos.dirty {
            self.cosmos.dirty = false;
            self.ipc_broadcast_state();
        }
    }
}

/// `COSMOS_DEBUG_ELEMENTS=1` — verbose per-frame element diagnostics used
/// by QEMU drive tests to separate "element not emitted" from "emitted
/// but occlusion-culled". Cached once; production runs pay one atomic
/// read per call site.
pub(crate) fn element_debug() -> bool {
    static DBG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DBG.get_or_init(|| std::env::var_os("COSMOS_DEBUG_ELEMENTS").is_some())
}

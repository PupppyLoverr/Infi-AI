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
    output::Output,
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

/// Every tiling state a snap can set — all are cleared before each
/// re-snap/unsnap so stale edge hints never linger on the client.
const SNAP_STATES: [xdg_toplevel::State; 5] = [
    xdg_toplevel::State::TiledLeft,
    xdg_toplevel::State::TiledRight,
    xdg_toplevel::State::TiledTop,
    xdg_toplevel::State::TiledBottom,
    xdg_toplevel::State::Maximized,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CosmosConfig {
    /// "dark" or "light"
    pub appearance: String,
    /// Logical output scale (1.0 = native). Recomputed per output from its
    /// width while `scale_auto` is set.
    pub scale: f64,
    /// Pick `scale` from the output width (see [`auto_scale`]). Cleared
    /// once the user sets a scale explicitly.
    pub scale_auto: bool,
    /// Disable window/workspace transition animation.
    pub reduce_motion: bool,
    /// Lite Mode: flat tinted surfaces instead of wallpaper glass, no
    /// animation and no layer drop shadows — for software rendering.
    pub lite_mode: bool,
    /// Accent preset name (see cosmos_ipc::ACCENT_PRESETS).
    pub accent: String,
    /// Dock rail position: "bottom" (default), "left", or "right".
    pub dock_position: String,
    /// Active wallpaper name — `<name>-<w>x<h>.png` under
    /// /usr/share/cosmos/wallpapers/.
    pub wallpaper: String,
    /// Clock and weather cards on the desktop (top-left).
    pub desktop_widgets: bool,
    /// Weather place name ("" = derive from the time zone city); geocoded
    /// via Open-Meteo by the shell.
    pub weather_city: String,
}

impl Default for CosmosConfig {
    fn default() -> Self {
        Self {
            appearance: "dark".to_string(),
            scale: 1.0,
            scale_auto: true,
            reduce_motion: false,
            lite_mode: false,
            accent: cosmos_ipc::DEFAULT_ACCENT.to_string(),
            dock_position: "bottom".to_string(),
            wallpaper: "violet".to_string(),
            desktop_widgets: true,
            weather_city: String::new(),
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
        // Persist the resolved accent rgbs too, so out-of-process uitk
        // apps get the wallpaper-extracted "auto" accent.
        match serde_json::to_string_pretty(&serde_json::Value::Object(self.as_map())) {
            Ok(text) => {
                if let Err(err) = std::fs::write(&path, text) {
                    warn!(?path, "failed to persist cosmos config: {err}");
                }
            }
            Err(err) => warn!("failed to serialize cosmos config: {err}"),
        }
    }

    /// Serialize to a JSON map for IPC `Config` events. Also injects
    /// the resolved accent rgbs so consumers (shell) paint the preset
    /// without shipping their own preset table.
    pub fn as_map(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut map = match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        let d = cosmos_ipc::accent_rgb(&self.accent, true);
        let l = cosmos_ipc::accent_rgb(&self.accent, false);
        map.insert(
            "accent_rgb".to_string(),
            serde_json::json!({ "dark": d, "light": l }),
        );
        map
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
                self.scale_auto = false;
            }
            "scale_auto" => {
                self.scale_auto = value
                    .as_bool()
                    .ok_or_else(|| "scale_auto must be a bool".to_string())?;
            }
            "desktop_widgets" => {
                self.desktop_widgets = value
                    .as_bool()
                    .ok_or_else(|| "desktop_widgets must be a bool".to_string())?;
            }
            "weather_city" => {
                let v = value
                    .as_str()
                    .ok_or_else(|| "weather_city must be a string".to_string())?;
                if v.chars().count() > 64 || v.chars().any(char::is_control) {
                    return Err("weather_city: up to 64 printable characters".to_string());
                }
                self.weather_city = v.trim().to_string();
            }
            "lite_mode" => {
                self.lite_mode = value
                    .as_bool()
                    .ok_or_else(|| "lite_mode must be a bool".to_string())?;
            }
            "reduce_motion" => {
                self.reduce_motion = value
                    .as_bool()
                    .ok_or_else(|| "reduce_motion must be a bool".to_string())?;
            }
            "accent" => {
                let v = value
                    .as_str()
                    .ok_or_else(|| "accent must be a preset name".to_string())?;
                if !cosmos_ipc::accent_known(v) {
                    return Err(format!("unknown accent preset: {v}"));
                }
                self.accent = v.to_string();
            }
            "wallpaper" => {
                let v = value
                    .as_str()
                    .ok_or_else(|| "wallpaper must be a name".to_string())?;
                if v.chars()
                    .any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                {
                    return Err("wallpaper: letters, digits, - and _ only".to_string());
                }
                self.wallpaper = v.to_string();
            }
            "dock_position" => {
                let v = value
                    .as_str()
                    .ok_or_else(|| "dock_position must be a string".to_string())?;
                if !matches!(v, "left" | "right" | "bottom") {
                    return Err(
                        "dock_position must be \"left\", \"right\" or \"bottom\"".to_string()
                    );
                }
                self.dock_position = v.to_string();
            }
            other => return Err(format!("unknown config key: {other}")),
        }
        Ok(())
    }
}

/// One aurora bloom: (u, v) center, gaussian sigma, blend strength, rgb.
pub type Bloom = (f32, f32, f32, f32, [f32; 3]);

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
    /// The one accent — active-state colour for compositor-side surfaces
    /// (snap drop-zone outline).
    pub accent: [f32; 4],
    /// Aurora wallpaper base rgb (resolved for dark/light).
    pub bg_base: [f32; 3],
    /// Aurora wallpaper blooms for this preset (resolved, incl. the
    /// light-mode pastel lift).
    pub blooms: [Bloom; 3],
}

/// Dark-mode aurora bloom tables per accent preset. Light mode lifts
/// each bloom toward white programmatically (pastel).
fn blooms_for(accent: &str) -> ([f32; 3], [Bloom; 3]) {
    match accent {
        "ember" => (
            [0.086, 0.070, 0.066],
            [
                (0.30, 0.26, 0.30, 0.55, [0.55, 0.30, 0.12]), // amber
                (0.78, 0.70, 0.28, 0.40, [0.45, 0.16, 0.14]), // rust
                (0.62, 0.15, 0.22, 0.30, [0.40, 0.20, 0.26]), // rose whisper
            ],
        ),
        "forest" => (
            [0.058, 0.080, 0.072],
            [
                (0.30, 0.26, 0.30, 0.55, [0.10, 0.38, 0.24]), // pine
                (0.78, 0.70, 0.28, 0.40, [0.12, 0.45, 0.33]), // emerald
                (0.62, 0.15, 0.22, 0.30, [0.20, 0.34, 0.20]), // moss
            ],
        ),
        "violet" => (
            [0.072, 0.062, 0.100],
            [
                (0.30, 0.26, 0.30, 0.55, [0.35, 0.22, 0.58]), // violet
                (0.78, 0.70, 0.28, 0.40, [0.21, 0.25, 0.55]), // indigo
                (0.62, 0.15, 0.22, 0.30, [0.42, 0.20, 0.44]), // magenta
            ],
        ),
        "rose" => (
            [0.090, 0.062, 0.078],
            [
                (0.30, 0.26, 0.30, 0.55, [0.50, 0.22, 0.36]), // rose
                (0.78, 0.70, 0.28, 0.40, [0.32, 0.18, 0.42]), // plum
                (0.62, 0.15, 0.22, 0.30, [0.48, 0.24, 0.28]), // coral
            ],
        ),
        "mono" => (
            [0.070, 0.075, 0.082],
            [
                (0.30, 0.26, 0.30, 0.55, [0.22, 0.24, 0.28]), // slate
                (0.78, 0.70, 0.28, 0.40, [0.16, 0.17, 0.20]), // graphite
                (0.62, 0.15, 0.22, 0.30, [0.24, 0.24, 0.26]), // ash
            ],
        ),
        // "azure" and anything unknown — the original navy aurora.
        _ => (
            [16.0 / 255.0, 20.0 / 255.0, 31.0 / 255.0],
            [
                (0.30, 0.26, 0.30, 0.55, [0.21, 0.25, 0.55]), // indigo
                (0.78, 0.70, 0.28, 0.40, [0.09, 0.31, 0.38]), // teal
                (0.62, 0.15, 0.22, 0.30, [0.24, 0.18, 0.40]), // violet whisper
            ],
        ),
    }
}

/// Light-mode wallpaper: pale slate base + each bloom pulled toward
/// white (pastel) and softened a touch.
fn pastel(blooms: [Bloom; 3]) -> [Bloom; 3] {
    blooms.map(|(x, y, s, st, c)| (x, y, s, st * 0.75, c.map(|v| v + (1.0 - v) * 0.55)))
}

impl CosmosTheme {
    pub fn from_config(cfg: &CosmosConfig) -> Self {
        let rgb = cosmos_ipc::accent_rgb(&cfg.accent, cfg.appearance != "light");
        let accent = [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        ];
        let (base, blooms) = blooms_for(&cfg.accent);
        // SSD chrome uses the same v3 window/sidebar fills as uitk app
        // bodies, so titlebar and body read as one surface.
        let pal = cosmos_theme::palette(cfg.appearance != "light");
        let f = |c: cosmos_theme::Rgba| {
            [
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
                1.0,
            ]
        };
        if cfg.appearance == "light" {
            Self {
                dark: false,
                background: [0.89, 0.90, 0.92, 1.0],
                titlebar_bg_focused: f(pal.window),
                titlebar_bg: f(pal.sidebar),
                titlebar_fg: f(pal.text),
                button_hover: [0.86, 0.84, 0.92, 1.0],
                focus_ring: [0.76, 0.73, 0.85, 1.0],
                accent,
                bg_base: [0.918, 0.929, 0.957],
                blooms: pastel(blooms),
            }
        } else {
            Self {
                dark: true,
                background: [0.07, 0.075, 0.08, 1.0],
                titlebar_bg_focused: f(pal.window),
                titlebar_bg: f(pal.sidebar),
                titlebar_fg: f(pal.text),
                button_hover: f(pal.raised),
                focus_ring: [0.30, 0.27, 0.42, 1.0],
                accent,
                bg_base: base,
                blooms,
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
    /// 70/30 wide pair.
    LeftWide,
    RightWide,
    /// Thirds.
    ThirdLeft,
    ThirdMid,
    ThirdRight,
    /// Quarter tiles (2×2 grid).
    QuarterTL,
    QuarterTR,
    QuarterBL,
    QuarterBR,
}

impl SnapState {
    /// Zone name accepted from the zoom-flyout IPC pick — the same
    /// names `cosmos_ipc::SNAP_LAYOUTS` advertises to the shell.
    pub fn from_zone(name: &str) -> Option<Self> {
        Some(match name {
            "left" => Self::Left,
            "right" => Self::Right,
            "max" => Self::Maximized,
            "left-wide" => Self::LeftWide,
            "right-wide" => Self::RightWide,
            "third-left" => Self::ThirdLeft,
            "third-mid" => Self::ThirdMid,
            "third-right" => Self::ThirdRight,
            "top-left" => Self::QuarterTL,
            "top-right" => Self::QuarterTR,
            "bottom-left" => Self::QuarterBL,
            "bottom-right" => Self::QuarterBR,
            _ => return None,
        })
    }

    /// The frame rect (loc + size INCLUDING the titlebar band) this
    /// snap occupies inside `area` — the single geometry table behind
    /// `snap_window` and the drag-edge drop preview.
    pub fn rect(
        self,
        area: Rectangle<i32, Logical>,
    ) -> Option<(Point<i32, Logical>, Size<i32, Logical>)> {
        let (w, h) = (area.size.w, area.size.h);
        let g = SNAP_GAP;
        let (loc, size) = match self {
            Self::Floating => return None,
            Self::Left => (area.loc + Point::from((g, g)), ((w / 2 - g * 2, h - g * 2))),
            Self::Right => (
                area.loc + Point::from((w / 2 + g, g)),
                ((w / 2 - g * 2, h - g * 2)),
            ),
            Self::Maximized => (area.loc, (w, h)),
            Self::LeftWide => (
                area.loc + Point::from((g, g)),
                ((w * 7 / 10 - g * 2, h - g * 2)),
            ),
            Self::RightWide => (
                area.loc + Point::from((w * 7 / 10 + g, g)),
                ((w * 3 / 10 - g * 2, h - g * 2)),
            ),
            Self::ThirdLeft => (area.loc + Point::from((g, g)), ((w / 3 - g * 2, h - g * 2))),
            Self::ThirdMid => (
                area.loc + Point::from((w / 3 + g, g)),
                ((w / 3 - g * 2, h - g * 2)),
            ),
            Self::ThirdRight => (
                area.loc + Point::from((w * 2 / 3 + g, g)),
                ((w / 3 - g * 2, h - g * 2)),
            ),
            Self::QuarterTL => (
                area.loc + Point::from((g, g)),
                ((w / 2 - g * 2, h / 2 - g * 2)),
            ),
            Self::QuarterTR => (
                area.loc + Point::from((w / 2 + g, g)),
                ((w / 2 - g * 2, h / 2 - g * 2)),
            ),
            Self::QuarterBL => (
                area.loc + Point::from((g, h / 2 + g)),
                ((w / 2 - g * 2, h / 2 - g * 2)),
            ),
            Self::QuarterBR => (
                area.loc + Point::from((w / 2 + g, h / 2 + g)),
                ((w / 2 - g * 2, h / 2 - g * 2)),
            ),
        };
        Some((loc, Size::from(size)))
    }

    /// xdg-toplevel states clients should see for this snap — quarter
    /// tiles report both edge hints; thirds/wides report the dominant
    /// edge; ThirdMid reports none (its edges touch nothing).
    fn xdg_states(self) -> &'static [xdg_toplevel::State] {
        use xdg_toplevel::State as S;
        match self {
            Self::Left | Self::LeftWide | Self::ThirdLeft => &[S::TiledLeft],
            Self::Right | Self::RightWide | Self::ThirdRight => &[S::TiledRight],
            Self::Maximized => &[S::Maximized],
            Self::QuarterTL => &[S::TiledLeft, S::TiledTop],
            Self::QuarterTR => &[S::TiledRight, S::TiledTop],
            Self::QuarterBL => &[S::TiledLeft, S::TiledBottom],
            Self::QuarterBR => &[S::TiledRight, S::TiledBottom],
            Self::Floating | Self::ThirdMid => &[],
        }
    }
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
    /// Keybind cheatsheet overlay open (super+?). Shell owns visuals;
    /// this flag only syncs compositor ↔ shell toggle state.
    pub help_open: bool,
    /// Super+D "show desktop": ids of the windows we minimized on the
    /// first press so a second press restores exactly that set.
    pub show_desktop: Option<Vec<u64>>,
    /// Alt/Super+Tab switcher while the modifier is held.
    pub switcher: Option<SwitcherState>,
    /// Live drag-to-edge snap hint: the zone the pointer targets and the
    /// exact rectangle the window would snap into on release (Win11's
    /// translucent drop preview).
    pub snap_preview: Option<(SnapState, Rectangle<i32, Logical>)>,
    /// Snap Assist picker while it is offered (shell renders visuals).
    pub assist: Option<AssistState>,
    /// Zoom flyout (Win11 snap layouts on green-button hover): the
    /// window id it's open for. Shell renders the card.
    pub zoom_flyout: Option<u64>,
    /// Armed dwell timer: (window id, since). The calloop timer
    /// re-checks the pointer zone at fire time — no cancel token.
    pub zoom_dwell: Option<(u64, std::time::Instant)>,
    /// Live transition animations (map-in, minimize-out, ws slide, cards).
    pub anims: crate::anim::Animations,
    /// Per-workspace dynamic tiling (master+stack). `super+t` toggles it
    /// for the active workspace.
    pub tiling: [bool; WORKSPACE_COUNT],
    /// ext-session-lock: while locked every normal client stays hidden
    /// and only `lock_surfaces` composite (plus the wallpaper behind).
    pub session_locked: bool,
    /// ext-session-lock surfaces handed to the lock client, one per
    /// output — `(surface, output it was created for)`.
    pub lock_surfaces: Vec<(smithay::wayland::session_lock::LockSurface, Output)>,
    /// The `ext_session_lock_v1` currently holding the lock. A second
    /// locker is refused while it is alive; once it dies the session
    /// stays locked and a replacement may take over.
    pub lock_owner: Option<
        smithay::reexports::wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1,
    >,
    /// Last compositor spawn of `cosmos-lock` — respawns are rate-limited
    /// so a crash-looping locker can't fork-storm the session.
    pub locker_spawned_at: Option<std::time::Instant>,
    /// The locked-session watchdog timer is armed.
    pub lock_watchdog: bool,
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

/// Default logical scale for an output `width_px` wide: 100% up to 1366,
/// 150% from 2560, 125% between (1920×1080 → 1536×864 logical).
pub fn auto_scale(width_px: i32) -> f64 {
    if width_px <= 1366 {
        1.0
    } else if width_px >= 2560 {
        1.5
    } else {
        1.25
    }
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
            tracing::info!(id, ws, "cosmos: window parked");
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
            tracing::info!(id, ws, loc = ?loc, "cosmos: window unparked");
            self.space.map_element(w, loc, false);
        }
    }

    /// Switch the active virtual desktop.
    pub fn switch_workspace(&mut self, target: usize) {
        if target >= WORKSPACE_COUNT || target == self.cosmos.active_workspace {
            return;
        }
        // A slide that is still mid-flight holds its outgoing windows
        // mapped — finish it synchronously before re-arranging, or the
        // next switch would park them onto the wrong desktop.
        self.finish_ws_slide();
        self.tick_animations();
        let from = self.cosmos.active_workspace;
        self.close_snap_assist();
        self.close_zoom_flyout();
        if self.anim_on() {
            // Deferred park: outgoing windows stay mapped and slide out
            // while the incoming desktop slides in — the macOS/Win11
            // desktop-switch read. `tick_animations` parks them at the
            // end of the animation.
            let leavers: Vec<u64> = self
                .space
                .elements()
                .map(|w| self.cosmos.window_id(w))
                .collect();
            if leavers.is_empty() {
                self.park_current(from);
            } else {
                for w in self.space.elements() {
                    let loc = self.space.element_location(w).unwrap_or_default();
                    let id = self.cosmos.window_id(w);
                    if let Some(meta) = self.cosmos.windows.get_mut(&id) {
                        meta.workspace = from;
                        meta.parked_loc = loc;
                    }
                    w.0.set_activated(false);
                }
            }
            // Even with no leavers the incoming desktop slides in.
            self.cosmos.anims.ws = Some(crate::anim::WsSlide {
                start: std::time::Instant::now(),
                dir: if target > from { 1 } else { -1 },
                leaving: leavers,
            });
            self.schedule_anim_tick();
        } else {
            self.park_current(from);
        }
        self.cosmos.active_workspace = target;
        self.unpark(target);
        // Reflow incoming windows when the target desktop tiles.
        self.retile_workspace();

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
        // The anchor window is leaving the screen — close the card.
        if self.cosmos.zoom_flyout == Some(id) {
            self.close_zoom_flyout();
        }
        tracing::info!(id, ws, "cosmos: window moved to workspace");
        if let Some(meta) = self.cosmos.windows.get_mut(&id) {
            meta.workspace = ws;
            meta.parked_loc = loc;
        }
        self.cosmos
            .parked
            .entry(ws)
            .or_default()
            .push(window.clone());
        self.retile_workspace();
        self.cosmos.dirty = true;
    }

    /// Minimize a window to the task area. With motion enabled the
    /// window stays mapped while it shrinks/fades out — the actual park
    /// happens in `tick_animations` when the animation expires.
    pub fn minimize_window(&mut self, window: &WindowElement) {
        let id = self.cosmos.window_id(window);
        let loc = self.space.element_location(window).unwrap_or_default();
        window.0.set_activated(false);
        tracing::info!(id, "cosmos: window minimized");
        let animate = self.anim_on();
        if let Some(meta) = self.cosmos.windows.get_mut(&id) {
            meta.minimized = true;
            meta.parked_loc = loc;
        }
        if animate {
            self.cosmos.anims.windows.insert(
                id,
                (std::time::Instant::now(), crate::anim::WinAnim::MinimizeOut),
            );
            self.schedule_anim_tick();
        } else {
            self.space.unmap_elem(window);
            // Park on the "minimized" lane, indexed at WORKSPACE_COUNT.
            self.cosmos
                .parked
                .entry(WORKSPACE_COUNT)
                .or_default()
                .push(window.clone());
        }
        // Move focus + activation to whatever is next.
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().unwrap();
        let top = self.space.elements().last().cloned();
        if let Some(top) = &top {
            self.space.raise_element(top, true);
        }
        keyboard.set_focus(self, top.map(KeyboardFocusTarget::from), serial);
        self.flush_pending_configures();
        self.retile_workspace();
        self.cosmos.dirty = true;
    }

    /// Restore a minimized window.
    pub fn unminimize_window(&mut self, id: u64) {
        // A minimize whose shrink animation is still running never
        // parked — cancel the anim and keep the window where it is.
        if let Some((_, kind)) = self.cosmos.anims.windows.get(&id).copied() {
            if kind == crate::anim::WinAnim::MinimizeOut {
                self.cosmos.anims.windows.remove(&id);
                if let Some(meta) = self.cosmos.windows.get_mut(&id) {
                    meta.minimized = false;
                }
                // Still mapped — raise, focus and re-activate it like a
                // normal restore.
                if let Some(window) = self.window_by_id(id) {
                    self.space.raise_element(&window, true);
                    window.0.set_activated(true);
                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                    let keyboard = self.seat.get_keyboard().unwrap();
                    keyboard.set_focus(self, Some(KeyboardFocusTarget::from(window)), serial);
                    self.flush_pending_configures();
                }
                self.cosmos.dirty = true;
                return;
            }
        }
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
            if self.anim_on() {
                self.cosmos
                    .anims
                    .windows
                    .insert(id, (std::time::Instant::now(), crate::anim::WinAnim::MapIn));
                self.schedule_anim_tick();
            }
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let keyboard = self.seat.get_keyboard().unwrap();
            keyboard.set_focus(self, Some(KeyboardFocusTarget::from(entry)), serial);
            self.flush_pending_configures();
            self.retile_workspace();
        } else {
            self.cosmos.parked.entry(ws).or_default().push(entry);
        }
        self.cosmos.dirty = true;
    }

    /// Motion enabled? Off under `reduce_motion` or `lite_mode`.
    pub fn anim_on(&self) -> bool {
        !self.cosmos.config.reduce_motion && !self.cosmos.config.lite_mode
    }

    /// Complete a workspace slide that is mid-flight: park every window
    /// that was sliding out onto its own desktop. Called synchronously at
    /// the top of `switch_workspace` and at animation expiry.
    pub fn finish_ws_slide(&mut self) {
        let Some(anim) = self.cosmos.anims.ws.take() else {
            return;
        };
        for id in anim.leaving {
            let Some(window) = self.window_by_id(id) else {
                continue;
            };
            // Already gone (destroyed mid-slide) or already parked.
            if !self.space.elements().any(|w| w == &window) {
                continue;
            }
            // A window whose minimize-out is still in flight belongs to
            // the minimized lane — parking it on its workspace lane would
            // strand it where `unminimize_window` never looks.
            let (minimized, ws) = self
                .cosmos
                .windows
                .get(&id)
                .map(|m| (m.minimized, m.workspace))
                .unwrap_or((false, self.cosmos.active_workspace));
            let lane = if minimized { WORKSPACE_COUNT } else { ws };
            self.space.unmap_elem(&window);
            self.cosmos.parked.entry(lane).or_default().push(window);
        }
        self.cosmos.dirty = true;
    }

    /// Park a window whose minimize-out animation finished.
    fn park_minimized(&mut self, id: u64) {
        let Some(window) = self.window_by_id(id) else {
            return;
        };
        if !self.space.elements().any(|w| w == &window) {
            return;
        }
        self.space.unmap_elem(&window);
        self.cosmos
            .parked
            .entry(WORKSPACE_COUNT)
            .or_default()
            .push(window);
        self.cosmos.dirty = true;
    }

    /// Advance animation bookkeeping: completes deferred unmaps
    /// (minimize-out, workspace slide) and prunes finished entrances.
    /// Called at the top of every rendered frame and from a fallback
    /// timer so a stalled renderer can't strand a pending park.
    pub fn tick_animations(&mut self) {
        let now = std::time::Instant::now();
        for id in self.cosmos.anims.expired_minimizes(now) {
            self.cosmos.anims.windows.remove(&id);
            self.park_minimized(id);
        }
        if self.cosmos.anims.ws_expired(now) {
            self.finish_ws_slide();
        }
        self.cosmos.anims.prune(now);
    }

    /// Keep a fallback timer alive while any deferred-park animation is
    /// pending — rendering normally outpaces this, but the timer is what
    /// guarantees `tick_animations` runs even on a stalled renderer.
    pub fn schedule_anim_tick(&mut self) {
        let _ = self.handle.insert_source(
            smithay::reexports::calloop::timer::Timer::from_duration(
                std::time::Duration::from_millis(24),
            ),
            |_, _, data| {
                data.tick_animations();
                if data.cosmos.anims.needs_timer() {
                    data.schedule_anim_tick();
                }
                smithay::reexports::calloop::timer::TimeoutAction::Drop
            },
        );
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
        snap.rect(area).map(|(loc, size)| Rectangle::new(loc, size))
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
        // A band centred on each zone edge — EDGE px of overshoot into the
        // shell chrome keeps the menubar fix working (drags crossing the
        // bar still snap), but a drag sitting deep inside an exclusive
        // strip like the left dock must not fire.
        if (ax - EDGE..=ax + EDGE).contains(&loc.x) {
            return Some(SnapState::Left);
        }
        if (ax + aw - EDGE..=ax + aw + EDGE).contains(&loc.x) {
            return Some(SnapState::Right);
        }
        if (ay - EDGE..=ay + EDGE).contains(&loc.y) {
            return Some(SnapState::Maximized);
        }
        None
    }

    /// Gap between tiled windows (logical px) — Hyprland's rhythm.
    const TILE_GAP: i32 = 8;
    /// Master pane width as a fraction of the work area.
    const TILE_MASTER: f64 = 0.58;

    /// Toggle dynamic tiling for the active workspace. On: every
    /// non-placed window reflows into master+stack and dragged-out
    /// windows rejoin. Off: members stay where they are as plain floats.
    pub fn toggle_tiling(&mut self) {
        let ws = self.cosmos.active_workspace;
        self.cosmos.tiling[ws] = !self.cosmos.tiling[ws];
        let on = self.cosmos.tiling[ws];
        tracing::info!(ws, on, "cosmos: tiling toggled");
        if on {
            for w in self.space.elements() {
                if let Some(fit) = w.user_data().get::<crate::shell::InitialFit>() {
                    fit.reset();
                }
            }
            self.retile_workspace();
        } else {
            // Keep the tile rects as ordinary floats — the initial-fit
            // clamp would otherwise keep nudging them.
            for w in self.space.elements() {
                if let Some(fit) = w.user_data().get::<crate::shell::InitialFit>() {
                    fit.user_moved();
                }
            }
        }
    }

    /// Reflow the active workspace into a master+stack layout: the
    /// frontmost window takes the tall master pane on the left, the rest
    /// split the right column evenly — Hyprland/Omarchy's daily-driver
    /// arrangement, with gaps. Windows the user placed themselves
    /// (drag, snap, maximize) are members no longer and keep their
    /// floating geometry.
    pub fn retile_workspace(&mut self) {
        if !self.cosmos.tiling[self.cosmos.active_workspace] {
            return;
        }
        // Front→back order: the focused/newest window owns the master
        // pane, matching every tiling-wm mental model.
        let members: Vec<(u64, WindowElement)> = self
            .space
            .elements()
            .rev()
            .map(|w| (self.cosmos.window_id(w), w.clone()))
            .collect();
        let members: Vec<(u64, WindowElement)> = members
            .into_iter()
            .filter(|(_, w)| {
                !w.user_data()
                    .get::<crate::shell::InitialFit>()
                    .map(|f| f.moved())
                    .unwrap_or(false)
            })
            .filter(|(id, _)| {
                !self
                    .cosmos
                    .windows
                    .get(id)
                    .map(|m| m.minimized)
                    .unwrap_or(false)
            })
            .collect();
        let n = members.len() as i32;
        if n == 0 {
            return;
        }
        let area = self.work_area(None);
        let g = Self::TILE_GAP;
        let master_w = if n > 1 {
            ((area.size.w - g * 3) as f64 * Self::TILE_MASTER) as i32
        } else {
            area.size.w - g * 2
        };
        for (i, (_, w)) in members.iter().enumerate() {
            let rect = if i == 0 {
                Rectangle::new(
                    area.loc + Point::from((g, g)),
                    Size::from((master_w, area.size.h - g * 2)),
                )
            } else {
                let sx = area.loc.x + master_w + g * 2;
                let sw = area.loc.x + area.size.w - sx - g;
                let sh = (area.size.h - g * 2 - (n - 2) * g) / (n - 1);
                let sy = area.loc.y + g + (i as i32 - 1) * (sh + g);
                Rectangle::new(Point::from((sx, sy)), Size::from((sw, sh)))
            };
            let Some(surface) = w.0.toplevel() else {
                continue;
            };
            let titlebar = w.titlebar_height();
            surface.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Maximized);
                s.states.unset(xdg_toplevel::State::TiledTop);
                s.states.unset(xdg_toplevel::State::TiledBottom);
                s.states.unset(xdg_toplevel::State::TiledLeft);
                s.states.unset(xdg_toplevel::State::TiledRight);
                s.states.set(if i == 0 {
                    xdg_toplevel::State::TiledLeft
                } else {
                    xdg_toplevel::State::TiledRight
                });
                s.size = Some(Size::from((rect.size.w, (rect.size.h - titlebar).max(1))));
            });
            surface.send_pending_configure();
            self.space.map_element(w.clone(), rect.loc, false);
        }
        self.flush_pending_configures();
        self.cosmos.dirty = true;
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
        let Some((loc, frame)) = snap.rect(area) else {
            let restore = self
                .cosmos
                .windows
                .get(&id)
                .and_then(|m| m.restore)
                .unwrap_or(Rectangle::new(current_loc, current_size));
            surface.with_pending_state(|s| {
                for st in SNAP_STATES {
                    s.states.unset(st);
                }
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
        };
        let size = Size::from((frame.w, frame.h - titlebar));

        surface.with_pending_state(|s| {
            for st in SNAP_STATES {
                s.states.unset(st);
            }
            for st in snap.xdg_states() {
                s.states.set(*st);
            }
            s.size = Some(size);
        });
        surface.send_pending_configure();
        self.space.map_element(window.clone(), loc, true);
        if let Some(fit) = window.user_data().get::<crate::shell::InitialFit>() {
            fit.user_moved();
        }
        self.flush_pending_configures();
        // In tiling mode a snapped window leaves the layout — the rest
        // reflow around it.
        self.retile_workspace();
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

    /// Hover bookkeeping for the zoom flyout (Win11 snap layouts):
    /// called from the SSD motion/leave handlers with whether the
    /// pointer is over the green (zone 2) button. Entering the button
    /// arms a dwell timer; leaving disarms it.
    pub fn note_zoom_hover(&mut self, window: &WindowElement, on_green: bool) {
        let id = self.cosmos.window_id(window);
        if !on_green {
            if matches!(self.cosmos.zoom_dwell, Some((d, _)) if d == id) {
                self.cosmos.zoom_dwell = None;
            }
            return;
        }
        if self.cosmos.zoom_flyout == Some(id)
            || matches!(self.cosmos.zoom_dwell, Some((d, _)) if d == id)
        {
            return;
        }
        self.cosmos.zoom_dwell = Some((id, std::time::Instant::now()));
        let _ = self.handle.insert_source(
            smithay::reexports::calloop::timer::Timer::from_duration(
                std::time::Duration::from_millis(cosmos_ipc::ZOOM_FLYOUT_DELAY_MS),
            ),
            move |_, _, data| {
                data.zoom_dwell_fire(id);
                smithay::reexports::calloop::timer::TimeoutAction::Drop
            },
        );
    }

    /// Dwell-timer callback: the flyout opens only if the pointer is
    /// still resting on the green button of the same window.
    fn zoom_dwell_fire(&mut self, id: u64) {
        if !matches!(self.cosmos.zoom_dwell, Some((d, _)) if d == id) {
            return;
        }
        self.cosmos.zoom_dwell = None;
        if self.cosmos.zoom_flyout.is_some() {
            return;
        }
        let still_on = self
            .window_by_id(id)
            .map(|w| {
                let st = w.decoration_state();
                st.is_ssd && st.header_bar.zone_at_pointer() == Some(2)
            })
            .unwrap_or(false);
        if still_on {
            self.open_zoom_flyout(id);
        }
    }

    /// Open the snap-layouts flyout under the window's green button.
    fn open_zoom_flyout(&mut self, id: u64) {
        let Some(window) = self.window_by_id(id) else {
            return;
        };
        let Some(loc) = self.space.element_location(&window) else {
            return;
        };
        // Green-button screen position: macOS cluster starts at
        // BTN_CX0 and the maximize disc is index 2. Center the card
        // under it, just below the titlebar.
        let bx =
            loc.x + crate::shell::ssd::BTN_CX0 as i32 + (crate::shell::ssd::BTN_PITCH as i32) * 2;
        let by = loc.y + crate::shell::ssd::HEADER_BAR_HEIGHT;
        self.cosmos.zoom_flyout = Some(id);
        self.ipc_broadcast(&cosmos_ipc::Event::ZoomFlyout {
            open: true,
            window: id,
            x: bx,
            y: by,
        });
        tracing::info!(id, "cosmos: zoom flyout open");
    }

    /// Close the zoom flyout if open (idempotent).
    pub fn close_zoom_flyout(&mut self) {
        if self.cosmos.zoom_flyout.take().is_some() {
            self.ipc_broadcast(&cosmos_ipc::Event::ZoomFlyout {
                open: false,
                window: 0,
                x: 0,
                y: 0,
            });
        }
    }

    /// Flyout pick: snap `id` into the named zone and close the card.
    /// Left/Right picks chain into Snap Assist exactly like a
    /// keybind snap — the remaining half still begs to be filled.
    pub fn snap_to_zone(&mut self, id: u64, zone: &str) {
        self.close_zoom_flyout();
        let Some(snap) = SnapState::from_zone(zone) else {
            warn!(zone, "cosmos: unknown snap zone from flyout");
            return;
        };
        let Some(window) = self.window_by_id(id) else {
            return;
        };
        self.snap_window_with_assist(&window, snap, true);
        self.focus_window(&window);
    }

    /// Push every pending toplevel configure (activation changes, bounds)
    /// so clients ack them — the SSD focus discs read the *acked* state,
    /// and unfocused clients otherwise wait indefinitely for the next
    /// configure to repaint.
    pub fn flush_pending_configures(&mut self) {
        for window in self.space.elements() {
            // A toplevel that hasn't committed yet gets its initial
            // configure from ensure_initial_configure, with its geometry.
            if let Some(toplevel) = window.0.toplevel() {
                if toplevel.is_initial_configure_sent() {
                    toplevel.send_pending_configure();
                }
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
        crate::shell::ssd::set_wallpaper_name(&cosmos_ipc::wallpaper_for(
            &self.cosmos.config.wallpaper,
            self.cosmos.config.appearance != "light",
        ));
        // All titlebars repaint next frame.
        for window in self.all_windows() {
            if let Some(state) = window
                .user_data()
                .get::<std::cell::RefCell<crate::shell::ssd::WindowState>>()
            {
                state.borrow_mut().header_bar.invalidate();
            }
        }
        if key == "scale" || key == "scale_auto" {
            self.apply_configured_scale();
        }
        let ev = cosmos_ipc::Event::Config(self.cosmos.config.as_map());
        self.ipc_broadcast(&ev);
        Ok(())
    }

    /// Push `config.scale` to every connected output and reflow window
    /// positions. The fractional-scale manager propagates the new
    /// preferred scale to all surfaces on the next frame.
    fn apply_configured_scale(&mut self) {
        use smithay::output::Scale;
        let outputs: Vec<smithay::output::Output> = self.space.outputs().cloned().collect();
        if self.cosmos.config.scale_auto {
            if let Some(mode) = outputs.first().and_then(|o| o.current_mode()) {
                self.cosmos.config.scale = auto_scale(mode.size.w);
                self.cosmos.config.save();
            }
        }
        let scale = self.cosmos.config.scale;
        for output in &outputs {
            output.change_current_state(None, None, Some(Scale::Fractional(scale)), None);
            self.backend_data.reset_buffers(output);
        }
        crate::shell::fixup_positions(&mut self.space, self.pointer.current_location());
        self.cosmos.dirty = true;
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

/// Window bodies render by importing each attached shm buffer through the
/// decal `TextureBuffer` path with a per-frame content-hash cache. This is
/// the default because it self-heals the transparent-body wedge: a torn
/// mid-paint sample is re-uploaded the very next frame instead of being
/// cached against the commit counter until the client's next commit.
/// `COSMOS_SHM_ELEMENTS=0` falls back to smithay's
/// `WaylandSurfaceRenderElement`/`MultiTexture` machinery (A/B escape).
pub(crate) fn shm_elements() -> bool {
    static DBG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DBG.get_or_init(|| {
        std::env::var("COSMOS_SHM_ELEMENTS")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}

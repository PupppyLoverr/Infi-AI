//! CosmosOS IPC protocol.
//!
//! The compositor owns a unix socket at `$XDG_RUNTIME_DIR/cosmos-ipc.sock`
//! (falling back to `/tmp/cosmos-ipc-$UID.sock`). Shell components and apps
//! connect, send [`Request`]s, and receive [`Event`] broadcasts whenever
//! window, workspace or settings state changes.
//!
//! Framing: newline-delimited JSON.

use serde::{Deserialize, Serialize};

/// Default name of the IPC socket inside the runtime dir.
pub const SOCKET_NAME: &str = "cosmos-ipc.sock";

/// Resolve the socket path the compositor listens on.
pub fn socket_path() -> std::path::PathBuf {
    let name = std::env::var("COSMOS_IPC_SOCKET").unwrap_or_else(|_| SOCKET_NAME.to_string());
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return std::path::PathBuf::from(dir).join(name);
    }
    std::path::PathBuf::from("/tmp").join(format!("cosmos-ipc-{}.sock", unsafe { libc_uid() }))
}

#[inline]
unsafe fn libc_uid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

/// A window as the compositor sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowInfo {
    /// Compositor-assigned, stable while the window lives.
    pub id: u64,
    pub title: String,
    pub app_id: String,
    /// Workspace index the window is on (0-based).
    pub workspace: u8,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub focused: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    /// Output name the window primarily lives on.
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceInfo {
    /// Workspace index (0-based).
    pub id: u8,
    pub focused: bool,
    pub window_count: usize,
}

/// A `ping` answer — also useful for the smoke test.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pong {
    pub version: String,
    pub name: String,
}

/// Client → compositor requests. One JSON object per line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Identify the client and get a `Pong` back.
    Ping,
    /// Subscribe this connection to [`Event`] broadcasts.
    Subscribe,
    /// One-shot state queries.
    ListWindows,
    ListWorkspaces,
    /// Ask the compositor to focus a window.
    FocusWindow {
        id: u64,
    },
    /// Move a window to a workspace (0-based).
    MoveWindowToWorkspace {
        id: u64,
        workspace: u8,
    },
    /// Close a window politely (xdg close).
    CloseWindow {
        id: u64,
    },
    /// Minimize a window (the menubar's Window ▸ Minimize).
    MinimizeWindow {
        id: u64,
    },
    /// Switch the active workspace.
    SwitchWorkspace {
        workspace: u8,
    },
    /// Toggle the launcher overlay (shell-only shortcut surface).
    ToggleLauncher,
    /// Toggle the keybind-cheatsheet overlay (super+?). The shell sends
    /// this when it closes the sheet itself (Esc / backdrop click) so
    /// the compositor's flag stays in sync — mirrors ToggleLauncher.
    ToggleHelp,
    /// Pick a window in the Snap Assist overlay — it snaps into the
    /// free half opposite the window that triggered the assist.
    SnapAssistPick {
        id: u64,
    },
    /// Dismiss Snap Assist without picking (backdrop click, Esc).
    SnapAssistDismiss,
    /// Snap a window into a named snap-layout zone picked in the zoom
    /// flyout (Win11's snap-layouts card on the green button). `zone`
    /// is one of [`SNAP_ZONES`].
    SnapToZone {
        id: u64,
        zone: String,
    },
    /// Dismiss the zoom flyout without picking (backdrop click, Esc).
    ZoomFlyoutDismiss,
    /// Update a setting; persisted by the compositor.
    SetConfig {
        key: String,
        value: serde_json::Value,
    },
    /// Read all settings.
    GetConfig,
    /// Render the (first) output offscreen and write it to `path` as a
    /// PNG. Replies `Event::Screenshot` on success, `Event::Error`
    /// otherwise. Backs the xdg-desktop-portal Screenshot impl and the
    /// drive harness's pixel evidence.
    Screenshot {
        path: String,
    },
    /// Ask the session to end (logout).
    QuitSession,
}

/// Compositor → client events, broadcast to subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Pong {
        pong: Pong,
    },
    Windows {
        windows: Vec<WindowInfo>,
    },
    Workspaces {
        workspaces: Vec<WorkspaceInfo>,
    },
    /// Full config dump after a `GetConfig` or a change.
    Config(serde_json::Map<String, serde_json::Value>),
    /// Something external toggled the launcher.
    LauncherToggled {
        open: bool,
    },
    /// Something external toggled the keybind cheatsheet.
    HelpToggled {
        open: bool,
    },
    /// Alt/Super+Tab window switcher state. `order` is the stable window-id
    /// order the compositor cycles through; `selected` is the highlighted id.
    /// `open: false` means the modifier was released and the choice committed.
    Switcher {
        open: bool,
        selected: u64,
        order: Vec<u64>,
    },
    /// Snap Assist state (Win11): after a Left/Right snap the picker
    /// offers the remaining windows to fill the free half. `fill` is
    /// "left" or "right" — the half the picker and the picked window
    /// occupy. `candidates` are eligible window ids. `open: false`
    /// closes the overlay.
    SnapAssist {
        open: bool,
        fill: String,
        candidates: Vec<u64>,
    },
    /// Zoom flyout (Win11 snap layouts on green-button hover). `x`,`y`
    /// is the screen-space anchor just under the button; the shell
    /// positions the card relative to it. `open: false` closes it.
    ZoomFlyout {
        open: bool,
        window: u64,
        x: i32,
        y: i32,
    },
    /// Reply to a `Screenshot` request — `path` was written as a PNG.
    Screenshot {
        path: String,
    },
    /// Session is ending; clients should exit.
    SessionEnding,
    /// A request failed — carries a human-readable reason.
    Error {
        message: String,
    },
}

/// Read one newline-delimited JSON message from `reader`.
pub fn read_message<T: serde::de::DeserializeOwned, R: std::io::BufRead>(
    reader: &mut R,
) -> std::io::Result<Option<T>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match serde_json::from_str(trimmed) {
        Ok(v) => Ok(Some(v)),
        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
    }
}

/// Write one newline-delimited JSON message to `writer`.
pub fn write_message<T: serde::Serialize, W: std::io::Write>(
    writer: &mut W,
    msg: &T,
) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, msg)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Snap-layout zones the zoom flyout offers — the names accepted by
/// `Request::SnapToZone`. Each entry is (zone name, fractional rect of
/// the work area) so both the compositor's geometry table and the
/// shell's thumbnail renderer share one source of truth.
pub const SNAP_LAYOUTS: &[(&str, &[(&str, f32, f32, f32, f32)])] = &[
    (
        "Halves",
        &[("left", 0.0, 0.0, 0.5, 1.0), ("right", 0.5, 0.0, 0.5, 1.0)],
    ),
    (
        "Thirds",
        &[
            ("third-left", 0.0, 0.0, 1.0 / 3.0, 1.0),
            ("third-mid", 1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
            ("third-right", 2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
        ],
    ),
    (
        "Quarters",
        &[
            ("top-left", 0.0, 0.0, 0.5, 0.5),
            ("top-right", 0.5, 0.0, 0.5, 0.5),
            ("bottom-left", 0.0, 0.5, 0.5, 0.5),
            ("bottom-right", 0.5, 0.5, 0.5, 0.5),
        ],
    ),
    (
        "Wide pair",
        &[
            ("left-wide", 0.0, 0.0, 0.7, 1.0),
            ("right-wide", 0.7, 0.0, 0.3, 1.0),
        ],
    ),
    ("Maximize", &[("max", 0.0, 0.0, 1.0, 1.0)]),
];

/// All zone names across `SNAP_LAYOUTS` (for validation).
pub fn zone_known(name: &str) -> bool {
    SNAP_LAYOUTS
        .iter()
        .any(|(_, cells)| cells.iter().any(|(n, ..)| *n == name))
}

/// Hover dwell on the green zoom button before the flyout opens.
pub const ZOOM_FLYOUT_DELAY_MS: u64 = 450;

/// Curated accent presets — the Omarchy-style "theme" dial. Each is one
/// accent color reserved for active state; every surface that honors
/// the accent resolves it through [`accent_rgb`], so a preset switch
/// repaints the whole desktop at once.
///
/// (name, label, dark-mode rgb, light-mode rgb)
pub const ACCENT_PRESETS: &[(&str, &str, [u8; 3], [u8; 3])] = &[
    ("azure", "Azure", [0x3D, 0x8B, 0xFF], [0x0A, 0x6E, 0xE8]),
    ("ember", "Ember", [0xFF, 0x9F, 0x2D], [0xC2, 0x57, 0x00]),
    ("forest", "Forest", [0x34, 0xC7, 0x59], [0x1F, 0x8A, 0x3D]),
    ("violet", "Violet", [0xA7, 0x8B, 0xFA], [0x7C, 0x3A, 0xED]),
    ("rose", "Rose", [0xF4, 0x72, 0xB6], [0xDB, 0x27, 0x77]),
    ("mono", "Mono", [0x9A, 0x9A, 0xA2], [0x6E, 0x6E, 0x73]),
];

/// Default accent preset name — "auto" follows the active wallpaper.
pub const DEFAULT_ACCENT: &str = "auto";

/// The accent extracted from the active wallpaper (set by the
/// compositor when it decodes the file). `None` until a wallpaper
/// has been sampled — `accent_rgb("auto")` falls back to Azure.
static WALLPAPER_ACCENT: std::sync::RwLock<Option<[u8; 3]>> = std::sync::RwLock::new(None);

/// Called by the compositor after decoding a wallpaper: stores the
/// dominant saturated colour that `"auto"` accent resolves to.
pub fn set_wallpaper_accent(rgb: [u8; 3]) {
    if let Ok(mut slot) = WALLPAPER_ACCENT.write() {
        *slot = Some(rgb);
    }
}

/// The last extracted wallpaper accent, if any.
/// File stem for the active wallpaper in this appearance: light mode
/// uses the pastel `<name>-light` variant shipped alongside each one.
pub fn wallpaper_for(name: &str, dark: bool) -> String {
    if dark || name.ends_with("-light") {
        name.to_string()
    } else {
        format!("{name}-light")
    }
}

pub fn wallpaper_accent() -> Option<[u8; 3]> {
    WALLPAPER_ACCENT.read().ok().and_then(|s| *s)
}

/// Resolve an accent preset to its (r,g,b) for `dark` mode.
/// `"auto"` resolves to the wallpaper's extracted dominant colour;
/// unknown names fall back to the default so a stale config can never
/// blank the accent.
pub fn accent_rgb(name: &str, dark: bool) -> [u8; 3] {
    if name == "auto" {
        if let Some(rgb) = wallpaper_accent() {
            return rgb;
        }
        return accent_rgb("azure", dark);
    }
    ACCENT_PRESETS
        .iter()
        .find(|(n, _, _, _)| *n == name)
        .or_else(|| ACCENT_PRESETS.iter().find(|(n, _, _, _)| *n == "azure"))
        .map(|(_, _, d, l)| if dark { *d } else { *l })
        .unwrap_or([0x3D, 0x8B, 0xFF])
}

/// Whether `name` is a known accent preset (`"auto"` included).
pub fn accent_known(name: &str) -> bool {
    name == "auto" || ACCENT_PRESETS.iter().any(|(n, _, _, _)| *n == name)
}

/// The user-facing keybind table — what the super+? cheatsheet renders.
/// Grouped (section, key, action) rows; keep in sync with
/// `compositor/src/input_handler.rs::process_keyboard_shortcut`.
pub const KEYBINDS: &[(&str, &str, &str)] = &[
    ("Apps", "Super + Return", "Terminal"),
    ("Apps", "Super + E", "File manager"),
    ("Apps", "Super + Space", "Launcher"),
    ("Apps", "Super + Q", "Close window"),
    ("Apps", "Super + F", "Fullscreen"),
    ("Apps", "Super + M", "Minimize"),
    (
        "Windows",
        "Super + \u{2190} / \u{2192}",
        "Snap left / right",
    ),
    (
        "Windows",
        "Super + \u{2191} / \u{2193}",
        "Maximize / restore",
    ),
    ("Windows", "Super + T", "Toggle tiling"),
    ("Windows", "Super + D", "Show desktop"),
    ("Windows", "Alt + Tab", "Window switcher"),
    ("Windows", "Drag to screen edge", "Snap preview"),
    ("Windows", "Double-click titlebar", "Maximize / restore"),
    ("Workspaces", "Super + 1\u{2013}9", "Switch workspace"),
    (
        "Workspaces",
        "Super + Shift + 1\u{2013}9",
        "Move window here",
    ),
    ("Session", "Ctrl + Alt + Backspace", "Log out"),
    ("Session", "Super + ?", "This cheatsheet"),
];

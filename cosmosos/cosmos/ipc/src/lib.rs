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
    /// Update a setting; persisted by the compositor.
    SetConfig {
        key: String,
        value: serde_json::Value,
    },
    /// Read all settings.
    GetConfig,
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

/// Default accent preset name.
pub const DEFAULT_ACCENT: &str = "azure";

/// Resolve an accent preset to its (r,g,b) for `dark` mode.
/// Unknown names fall back to the default so a stale config can never
/// blank the accent.
pub fn accent_rgb(name: &str, dark: bool) -> [u8; 3] {
    ACCENT_PRESETS
        .iter()
        .find(|(n, _, _, _)| *n == name)
        .or_else(|| {
            ACCENT_PRESETS
                .iter()
                .find(|(n, _, _, _)| *n == DEFAULT_ACCENT)
        })
        .map(|(_, _, d, l)| if dark { *d } else { *l })
        .unwrap_or([0x3D, 0x8B, 0xFF])
}

/// Whether `name` is a known accent preset.
pub fn accent_known(name: &str) -> bool {
    ACCENT_PRESETS.iter().any(|(n, _, _, _)| *n == name)
}

/// The user-facing keybind table — what the super+? cheatsheet renders.
/// Grouped (section, key, action) rows; keep in sync with
/// `compositor/src/input_handler.rs::process_keyboard_shortcut`.
pub const KEYBINDS: &[(&str, &str, &str)] = &[
    ("Apps", "Super + Return", "Terminal"),
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

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
    FocusWindow { id: u64 },
    /// Move a window to a workspace (0-based).
    MoveWindowToWorkspace { id: u64, workspace: u8 },
    /// Close a window politely (xdg close).
    CloseWindow { id: u64 },
    /// Switch the active workspace.
    SwitchWorkspace { workspace: u8 },
    /// Toggle the launcher overlay (shell-only shortcut surface).
    ToggleLauncher,
    /// Update a setting; persisted by the compositor.
    SetConfig { key: String, value: serde_json::Value },
    /// Read all settings.
    GetConfig,
    /// Ask the session to end (logout).
    QuitSession,
}

/// Compositor → client events, broadcast to subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Pong(Pong),
    Windows(Vec<WindowInfo>),
    Workspaces(Vec<WorkspaceInfo>),
    /// Full config dump after a `GetConfig` or a change.
    Config(serde_json::Map<String, serde_json::Value>),
    /// Something external toggled the launcher.
    LauncherToggled { open: bool },
    /// Session is ending; clients should exit.
    SessionEnding,
    /// A request failed — carries a human-readable reason.
    Error { message: String },
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

//! Compositor state, desktop app discovery, live IPC command socket, and
//! persisted shell preferences.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::canvas::{decode_png, Icon, Rgba};
use crate::tokens::ThemeKind;

pub const ISLAND_TTL_SECS: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockStyle {
    Solyra,
    Macos,
    Windows,
    /// Icons only: no start cell, no agent shelf, a smaller card.
    Minimal,
}

impl DockStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solyra => "solyra",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Minimal => "minimal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Solyra => "Solyra",
            Self::Macos => "Floating",
            Self::Windows => "Taskbar",
            Self::Minimal => "Minimal",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "solyra" | "hybrid" => Some(Self::Solyra),
            "macos" | "mac" => Some(Self::Macos),
            "windows" | "win" | "taskbar" => Some(Self::Windows),
            "minimal" => Some(Self::Minimal),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperPreset {
    /// Unsplash beach photograph — the default splash.
    Beach,
    Violet,
    Ocean,
    Sunset,
    Forest,
    Graphite,
}

impl WallpaperPreset {
    pub const ALL: [Self; 6] = [
        Self::Beach,
        Self::Ocean,
        Self::Sunset,
        Self::Forest,
        Self::Graphite,
        Self::Violet,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Beach => "beach",
            Self::Violet => "violet",
            Self::Ocean => "ocean",
            Self::Sunset => "sunset",
            Self::Forest => "forest",
            Self::Graphite => "graphite",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Beach => "Beach",
            Self::Violet => "Violet",
            Self::Ocean => "Ocean",
            Self::Sunset => "Sunset",
            Self::Forest => "Forest",
            Self::Graphite => "Graphite",
        }
    }

    pub fn swatch(self) -> Rgba {
        match self {
            Self::Beach => Rgba::rgb(0x3b, 0x9e, 0xd8),
            Self::Violet => Rgba::rgb(0xa8, 0x55, 0xf7),
            Self::Ocean => Rgba::rgb(0x0e, 0xa5, 0xe9),
            Self::Sunset => Rgba::rgb(0xf9, 0x73, 0x16),
            Self::Forest => Rgba::rgb(0x10, 0xb9, 0x81),
            Self::Graphite => Rgba::rgb(0x64, 0x74, 0x8b),
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "beach" | "splash" => Some(Self::Beach),
            "violet" => Some(Self::Violet),
            "ocean" => Some(Self::Ocean),
            "sunset" => Some(Self::Sunset),
            "forest" => Some(Self::Forest),
            "graphite" => Some(Self::Graphite),
            _ => None,
        }
    }

    pub fn path(self) -> String {
        let usr = format!("/usr/share/solyra/wallpapers/{}.png", self.as_str());
        if std::path::Path::new(&usr).exists() {
            return usr;
        }
        format!("assets/wallpapers/{}-1920x1080.png", self.as_str())
    }
}

/// How a wallpaper image is fitted to the screen (swaybg's `-m`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperMode {
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
}

impl WallpaperMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Stretch => "stretch",
            Self::Center => "center",
            Self::Tile => "tile",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "fill" => Some(Self::Fill),
            "fit" => Some(Self::Fit),
            "stretch" => Some(Self::Stretch),
            "center" => Some(Self::Center),
            "tile" => Some(Self::Tile),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ShellCmd {
    /// (sending app, display line); an empty line dismisses the banner.
    Notify(String, String),
    ToggleLauncher,
    ToggleControl,
    ToggleIsland,
    ToggleAgents,
    /// Notification centre drawer (history of notifications).
    ToggleNotifs,
    ClearNotifs,
    /// Open the "About This Computer" card.
    About,
    CloseOverlays,
    SetDock(DockStyle),
    SetWallpaper(WallpaperPreset),
    /// Bring an auto-hidden dock back.
    RevealDock,
    /// Lock the screen (Solyra's lock screen, swaylock if it cannot start).
    Lock,
    /// Turn dock auto-hide on or off.
    ToggleDockAutohide,
    /// Show the island's shelf (true) or what is live (false).
    IslandShelf(bool),
    /// Forget shelf item `n` (the file itself is untouched).
    ShelfRemove(usize),
    /// Files dropped on the island.
    ShelfAdd(Vec<std::path::PathBuf>),
    /// A user-chosen image. The path is validated before use.
    SetWallpaperFile {
        mode: WallpaperMode,
        path: String,
    },
}

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub id: u64,
    pub app_id: String,
    pub title: String,
    #[allow(dead_code)]
    pub workspace_id: u64,
    pub is_focused: bool,
}

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub window: Option<u64>,
    pub window_count: usize,
    pub is_focused: bool,
    pub app_id: String,
    pub role: String,
    pub title: String,
    pub subtitle: String,
    pub command: LaunchTarget,
}

#[derive(Clone, Debug)]
pub enum LaunchTarget {
    Shell(String),
    Ipc(&'static str),
    Internal(ShellCmd),
}

#[derive(Clone, Debug)]
pub struct Telemetry {
    pub mem_used_mib: u64,
    pub mem_total_mib: u64,
    /// `None` when the machine has no battery. Never a placeholder number.
    pub battery_pct: Option<u8>,
    pub network: NetworkState,
    /// A backlight device exists and `brightnessctl` can drive it.
    pub brightness_controllable: bool,
    /// `wpctl` is installed, so the volume slider reaches the audio server.
    pub volume_controllable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkState {
    Wired,
    Wireless,
    Offline,
}

impl NetworkState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wired => "Wired · connected",
            Self::Wireless => "Wi-Fi · connected",
            Self::Offline => "Offline",
        }
    }
}

pub struct ShellPrefs {
    pub dock_style: DockStyle,
    pub wallpaper: WallpaperPreset,
    /// A user-chosen image; when set it wins over the preset.
    pub wallpaper_file: Option<String>,
    pub wallpaper_mode: WallpaperMode,
    pub theme: ThemeKind,
    /// Focus: while on, plain `notify` messages are held back. Agent
    /// approvals are never muted.
    pub dnd_on: bool,
    pub volume: u8,
    pub brightness: u8,
    /// Bar, notch, dock behaviour, clock, accent and lock style, owned by
    /// Settings (`solyra_conf::look`). The panel only reads these.
    pub look: solyra_conf::look::Look,
    /// The wallpaper's average colour (Acrylic's tint). Worked out by the
    /// panel when the wallpaper changes; not stored.
    pub wall_tint: Option<crate::tokens::Rgba>,
    /// Agent desk environment: 0 Cloud, 1 Local, 2 Worktree (`agent_env`).
    pub agent_env: u8,
}

impl Default for ShellPrefs {
    fn default() -> Self {
        Self {
            dock_style: DockStyle::Macos,
            wallpaper: WallpaperPreset::Beach,
            wallpaper_file: None,
            wallpaper_mode: WallpaperMode::Fill,
            theme: ThemeKind::Dark,
            dnd_on: false,
            volume: 76,
            brightness: 84,
            look: solyra_conf::look::Look::default(),
            wall_tint: None,
            agent_env: 1,
        }
    }
}

/// `agent_env` in shell.conf: `cloud`, `local`, or `worktree`.
pub fn parse_agent_env(raw: &str) -> Option<u8> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "cloud" | "0" => Some(0),
        "local" | "1" => Some(1),
        "worktree" | "2" => Some(2),
        _ => None,
    }
}

pub fn agent_env_name(which: u8) -> &'static str {
    match which {
        0 => "cloud",
        2 => "worktree",
        _ => "local",
    }
}

impl ShellPrefs {
    /// The image on the desktop now: the person's own file, else the preset.
    pub fn wallpaper_image_path(&self) -> String {
        self.wallpaper_file
            .clone()
            .unwrap_or_else(|| self.wallpaper.path())
    }

    /// Work out Acrylic's tint from the current wallpaper (PNG presets and
    /// PNG files; other formats keep the theme colour).
    pub fn compute_wall_tint(&mut self) {
        self.wall_tint = std::fs::read(self.wallpaper_image_path())
            .ok()
            .and_then(|b| crate::canvas::decode_png(&b))
            .and_then(|img| crate::material::average(&img));
    }

    fn config_path() -> Option<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg).join("solyra/shell.conf"));
        }
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".config/solyra/shell.conf"))
    }

    pub fn load() -> Self {
        let mut prefs = Self::default();
        let Some(path) = Self::config_path() else {
            return prefs;
        };
        let Ok(raw) = std::fs::read_to_string(path) else {
            return prefs;
        };
        prefs.look = solyra_conf::look::Look::from_conf(&raw);
        for line in raw.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "dock_style" => {
                    if let Some(d) = DockStyle::parse(v) {
                        prefs.dock_style = d;
                    }
                }
                "wallpaper" => {
                    if let Some(w) = WallpaperPreset::parse(v) {
                        prefs.wallpaper = w;
                    }
                }
                "wallpaper_file" => {
                    let v = v.trim();
                    if !v.is_empty() {
                        prefs.wallpaper_file = Some(v.to_string());
                    }
                }
                "wallpaper_mode" => {
                    if let Some(m) = WallpaperMode::parse(v) {
                        prefs.wallpaper_mode = m;
                    }
                }
                "theme" => {
                    if let Some(t) = ThemeKind::parse(v) {
                        prefs.theme = t;
                    }
                }
                "volume" => {
                    if let Ok(n) = v.trim().parse::<u8>() {
                        prefs.volume = n.min(100);
                    }
                }
                "dnd" => prefs.dnd_on = v.trim() == "1",
                "brightness" => {
                    if let Ok(n) = v.trim().parse::<u8>() {
                        prefs.brightness = n.clamp(15, 100);
                    }
                }
                "agent_env" => {
                    if let Some(n) = parse_agent_env(v) {
                        prefs.agent_env = n;
                    }
                }
                _ => {}
            }
        }
        prefs
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                eprintln!("solyra-panel: cannot create {}: {err}", parent.display());
                return;
            }
        }
        let pairs: Vec<(&str, Option<String>)> = vec![
            ("dock_style", Some(self.dock_style.as_str().into())),
            ("wallpaper", Some(self.wallpaper.as_str().into())),
            ("wallpaper_file", self.wallpaper_file.clone()),
            ("wallpaper_mode", Some(self.wallpaper_mode.as_str().into())),
            ("theme", Some(self.theme.as_str().into())),
            ("volume", Some(self.volume.to_string())),
            ("brightness", Some(self.brightness.to_string())),
            ("agent_env", Some(agent_env_name(self.agent_env).into())),
            ("dnd", Some(if self.dnd_on { "1" } else { "0" }.into())),
        ];
        if let Err(err) = solyra_conf::update(&path, &pairs) {
            eprintln!("solyra-panel: cannot save {}: {err}", path.display());
        }
    }

    /// Last-modified time of the config file, to notice edits made elsewhere
    /// (the Settings app).
    pub fn config_mtime() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::config_path()?)
            .ok()?
            .modified()
            .ok()
    }
}

pub fn runtime_path(name: &str) -> Option<PathBuf> {
    let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(PathBuf::from(dir).join(name))
}

pub fn send_client_cmd(line: &str) -> std::io::Result<()> {
    // Also mirror notifications to `solyra-island` for compatibility.
    if let Some(msg) = line.strip_prefix("notify ") {
        if let Some(path) = runtime_path("solyra-island") {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = std::fs::write(path, format!("{now}\t{msg}\n"));
        }
    }
    let sock_path = runtime_path("solyra-shell.sock").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set")
    })?;
    if let Ok(mut stream) = UnixStream::connect(&sock_path) {
        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        return Ok(());
    }
    Ok(())
}

pub fn spawn_ipc_listener<F>(queue: Arc<Mutex<Vec<ShellCmd>>>, wake: F)
where
    F: Fn() + Send + 'static,
{
    let Some(sock_path) = runtime_path("solyra-shell.sock") else {
        return;
    };
    let _ = std::fs::remove_file(&sock_path);
    let Ok(listener) = UnixListener::bind(&sock_path) else {
        return;
    };
    std::thread::spawn(move || {
        for incoming in listener.incoming().flatten() {
            let mut reader = BufReader::new(incoming);
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() {
                if let Some(cmd) = parse_cmd_line(&line) {
                    if let Ok(mut q) = queue.lock() {
                        q.push(cmd);
                    }
                    wake();
                }
            }
        }
    });
}

fn parse_cmd_line(raw: &str) -> Option<ShellCmd> {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix("notify ") {
        return Some(ShellCmd::Notify("Shell".into(), rest.trim().to_string()));
    }
    if let Some(rest) = s.strip_prefix("dock ") {
        return DockStyle::parse(rest).map(ShellCmd::SetDock);
    }
    if let Some(rest) = s.strip_prefix("wallpaper-file ") {
        // `wallpaper-file <mode> <path>`; the path may contain spaces.
        let (mode, path) = rest.trim().split_once(' ')?;
        let mode = WallpaperMode::parse(mode)?;
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        return Some(ShellCmd::SetWallpaperFile {
            mode,
            path: path.to_string(),
        });
    }
    if let Some(rest) = s.strip_prefix("wallpaper ") {
        return WallpaperPreset::parse(rest).map(ShellCmd::SetWallpaper);
    }
    match s {
        "toggle-launcher" | "launcher" => Some(ShellCmd::ToggleLauncher),
        "toggle-control" | "control" => Some(ShellCmd::ToggleControl),
        "toggle-island" | "island" => Some(ShellCmd::ToggleIsland),
        "toggle-agents" | "agents" => Some(ShellCmd::ToggleAgents),
        "about" => Some(ShellCmd::About),
        "toggle-notifs" | "notifs" => Some(ShellCmd::ToggleNotifs),
        "clear-notifs" => Some(ShellCmd::ClearNotifs),
        "close" => Some(ShellCmd::CloseOverlays),
        "lock" => Some(ShellCmd::Lock),
        _ => None,
    }
}

pub fn read_island_file() -> Option<String> {
    let path = runtime_path("solyra-island")?;
    let raw = std::fs::read_to_string(path).ok()?;
    let (stamp, text) = raw.split_once('\t')?;
    let stamp: u64 = stamp.trim().parse().ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(stamp) > ISLAND_TTL_SECS || text.trim().is_empty() {
        return None;
    }
    Some(text.trim().to_string())
}

/// Why a wallpaper file cannot be used, or `Ok` with its canonical path.
pub fn validate_wallpaper(path: &str) -> Result<PathBuf, String> {
    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    let p = PathBuf::from(path);
    let canon = p.canonicalize().map_err(|e| format!("{path}: {e}"))?;
    let meta = std::fs::metadata(&canon).map_err(|e| format!("{path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("{path} is larger than 64 MiB"));
    }
    let mut head = [0u8; 12];
    let n = std::fs::File::open(&canon)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("{path}: {e}"))?;
    let png = head[..n].starts_with(&[0x89, b'P', b'N', b'G']);
    let jpeg = head[..n].starts_with(&[0xff, 0xd8, 0xff]);
    if !(png || jpeg) {
        return Err(format!("{path} is not a PNG or JPEG image"));
    }
    Ok(canon)
}

/// Show `path` as the wallpaper. Starts the new `swaybg` first and then
/// retires the old one, so the desktop never flashes bare. No shell is
/// involved, so a file name cannot inject anything.
pub fn apply_wallpaper_file(path: &std::path::Path, mode: WallpaperMode) -> std::io::Result<()> {
    let child = std::process::Command::new("swaybg")
        .arg("-i")
        .arg(path)
        .args(["-m", mode.as_str()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    // Retire the older swaybg instances once the new one has had time to
    // draw. Everything except the process just started goes; the child id is
    // compared, so a lone instance is never killed.
    let keep = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let Ok(out) = std::process::Command::new("pgrep")
            .args(["-x", "swaybg"])
            .output()
        else {
            return;
        };
        for pid in String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|p| p.parse::<u32>().ok())
            .filter(|p| *p != keep)
        {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
        }
    });
    // The child is deliberately not waited on: swaybg runs until replaced.
    std::mem::forget(child);
    Ok(())
}

pub fn apply_wallpaper(preset: WallpaperPreset) {
    let path = preset.path();
    if let Err(err) = apply_wallpaper_file(std::path::Path::new(&path), WallpaperMode::Fill) {
        eprintln!("solyra-panel: cannot start swaybg: {err}");
    }
}

// ---------------------------------------------------------------- niri IPC

pub fn niri_socket() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NIRI_SOCKET") {
        return Some(PathBuf::from(path));
    }
    let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("niri.wayland-") && name.ends_with(".sock") {
            if let Ok(meta) = entry.metadata() {
                let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                    newest = Some((modified, entry.path()));
                }
            }
        }
    }
    newest.map(|(_, path)| path)
}

pub fn ipc(request: &str) -> Option<String> {
    let path = niri_socket()?;
    let mut stream = UnixStream::connect(path).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(350)))
        .ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    Some(reply)
}

pub fn send_ipc(request: &str) {
    let _ = ipc(request);
}

/// niri's reported version string, when the IPC socket answers.
pub fn compositor_version() -> Option<String> {
    let reply = ipc("\"Version\"")?;
    let v: serde_json::Value = serde_json::from_str(&reply).ok()?;
    v.get("Ok")?.get("Version")?.as_str().map(String::from)
}

pub fn workspaces() -> Vec<(u8, bool)> {
    let Some(reply) = ipc("\"Workspaces\"") else {
        return (1..=3).map(|i| (i, i == 1)).collect();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply) else {
        return (1..=3).map(|i| (i, i == 1)).collect();
    };
    // niri replies either `[...]` or `{"Ok":{"Workspaces":[...]}}` depending on build.
    let list = value
        .as_array()
        .or_else(|| value.get("Ok")?.get("Workspaces")?.as_array());
    let mut out = Vec::new();
    if let Some(list) = list {
        for ws in list {
            let idx = ws.get("idx").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let active = ws
                .get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if (1..=9).contains(&idx) {
                out.push((idx, active));
            }
        }
    }
    out.sort_by_key(|(idx, _)| *idx);
    out.dedup_by_key(|(idx, _)| *idx);
    if out.is_empty() {
        (1..=3).map(|i| (i, i == 1)).collect()
    } else {
        out
    }
}

pub fn windows() -> Vec<WindowInfo> {
    let Some(reply) = ipc("\"Windows\"") else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply) else {
        return Vec::new();
    };
    let list = value
        .as_array()
        .or_else(|| value.get("Ok")?.get("Windows")?.as_array());
    let mut out = Vec::new();
    if let Some(list) = list {
        for win in list {
            let Some(id) = win.get("id").and_then(|v| v.as_u64()) else {
                continue;
            };
            let app_id = win
                .get("app_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if app_id.is_empty() {
                continue;
            }
            let title = win
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(&app_id)
                .to_string();
            let workspace_id = win
                .get("workspace_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            let is_focused = win
                .get("is_focused")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            out.push(WindowInfo {
                id,
                app_id,
                title,
                workspace_id,
                is_focused,
            });
        }
    }
    out
}

pub fn focused_app_menus(wins: &[WindowInfo]) -> (String, &'static [&'static str]) {
    if let Some(w) = wins.iter().find(|w| w.is_focused) {
        let lower = w.app_id.to_ascii_lowercase();
        if lower.contains("thunar") || lower.contains("files") {
            return (
                "Files".into(),
                &["File", "Edit", "View", "Go", "Window", "Help"],
            );
        }
        if lower.contains("foot") || lower.contains("term") {
            return (
                "Terminal".into(),
                &["Shell", "Edit", "View", "Window", "Help"],
            );
        }
        return (
            pretty_app_name(&w.app_id),
            &["File", "Edit", "View", "Window", "Help"],
        );
    }
    ("Solyra".into(), &["File", "View", "Go", "Window", "Help"])
}

pub fn pretty_app_name(app_id: &str) -> String {
    let base = app_id.rsplit('.').next().unwrap_or(app_id);
    match base.to_ascii_lowercase().as_str() {
        "foot" => "Terminal".into(),
        "thunar" => "Files".into(),
        _ => {
            let mut s = base.to_string();
            if let Some(f) = s.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            s
        }
    }
}

pub fn role_for_app_id(app_id: &str) -> String {
    let lower = app_id.to_ascii_lowercase();
    if lower.contains("thunar") || lower.contains("files") {
        "files".into()
    } else if lower.contains("foot") || lower.contains("term") {
        "terminal".into()
    } else if lower.contains("solyra-browse") || lower.contains("firefox") {
        "browser".into()
    } else if lower.contains("mousepad") || lower.contains("texteditor") {
        "editor".into()
    } else if lower.contains("ristretto") || lower.contains("loupe") || lower.contains("eog") {
        "viewer".into()
    } else if lower.contains("galculator") || lower.contains("calculator") {
        "calculator".into()
    } else {
        lower
    }
}

pub fn role_binary(role: &str, fallback: &str) -> String {
    let mut paths = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(home).join(".config/solyra/defaults.toml"));
    }
    paths.push(PathBuf::from("/etc/solyra/defaults.toml"));
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == role {
                let binary = value.trim().trim_matches('"').trim_end_matches(".desktop");
                if !binary.is_empty() {
                    return binary.to_string();
                }
            }
        }
    }
    fallback.to_string()
}

/// Is `bin` on `PATH`? Pinned apps that are not installed are left out of the
/// dock rather than shown as icons that do nothing. Tests and previews see
/// every pin, so the dock renders the same everywhere.
pub fn pin_available(bin: &str) -> bool {
    cfg!(test) || on_path(bin)
}

/// The floating dock's pins, in the pearOS / macOS order: Files, then the
/// apps people use every day, Settings last. The launcher goes in after
/// Files (see `dock::items`).
pub fn floating_dock_entries(wins: &[WindowInfo]) -> Vec<AppEntry> {
    let pin = |role: &str, bin: String, title: &str, subtitle: &str, open: &str| AppEntry {
        window: None,
        window_count: 0,
        is_focused: false,
        app_id: bin,
        role: role.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        command: LaunchTarget::Shell(open.into()),
    };
    let candidates = vec![
        (role_binary("files", "solyra-files"), pin("files", role_binary("files", "solyra-files"), "Files", "Browse folders and drives", "solyra-open files")),
        (role_binary("browser", "firefox"), pin("browser", role_binary("browser", "firefox"), "Browser", "Browse the web", "solyra-open browser")),
        (role_binary("terminal", "foot"), pin("terminal", role_binary("terminal", "foot"), "Terminal", "Command line shell", "solyra-open terminal")),
        (role_binary("editor", "mousepad"), pin("editor", role_binary("editor", "mousepad"), "Text Editor", "Edit text files", "solyra-open editor")),
        (role_binary("viewer", "ristretto"), pin("viewer", role_binary("viewer", "ristretto"), "Image Viewer", "Look at photos and pictures", "solyra-open viewer")),
        (role_binary("calculator", "galculator"), pin("calculator", role_binary("calculator", "galculator"), "Calculator", "Do the maths", "solyra-open calculator")),
        ("solyra-settings".to_string(), pin("settings", "solyra-settings".into(), "System Settings", "Appearance, dock, network and more", "solyra-settings")),
    ];
    let items: Vec<AppEntry> = candidates
        .into_iter()
        .filter(|(bin, _)| pin_available(bin))
        .map(|(_, e)| e)
        .collect();
    merge_windows(items, wins)
}

/// Fold the open windows into pinned entries, appending apps that are running
/// but not pinned.
fn merge_windows(mut items: Vec<AppEntry>, wins: &[WindowInfo]) -> Vec<AppEntry> {
    for win in wins {
        let win_role = role_for_app_id(&win.app_id);
        if let Some(existing) = items
            .iter_mut()
            .find(|e| e.app_id.eq_ignore_ascii_case(&win.app_id) || e.role == win_role)
        {
            existing.window_count += 1;
            if existing.window.is_none() || win.is_focused {
                existing.window = Some(win.id);
            }
            existing.is_focused |= win.is_focused;
        } else {
            items.push(AppEntry {
                window: Some(win.id),
                window_count: 1,
                is_focused: win.is_focused,
                app_id: win.app_id.clone(),
                role: win_role,
                title: pretty_app_name(&win.app_id),
                subtitle: win.title.clone(),
                command: LaunchTarget::Shell(win.app_id.clone()),
            });
        }
    }
    items
}

pub fn dock_entries(wins: &[WindowInfo]) -> Vec<AppEntry> {
    let term_bin = role_binary("terminal", "foot");
    let files_bin = role_binary("files", "solyra-files");

    // Pinned core apps in deterministic order (just like macOS / Windows 11),
    // enriched with live window state, followed by any unpinned running apps.
    let mut items = vec![
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: files_bin.clone(),
            role: "files".into(),
            title: "Files".into(),
            subtitle: "Browse folders and drives".into(),
            command: LaunchTarget::Shell("solyra-open files".into()),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: term_bin.clone(),
            role: "terminal".into(),
            title: "Terminal".into(),
            subtitle: "Command line shell".into(),
            command: LaunchTarget::Shell("solyra-open terminal".into()),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-browse".into(),
            role: "browser".into(),
            title: "Browser".into(),
            subtitle: "Browse the web".into(),
            command: LaunchTarget::Shell("solyra-open browser".into()),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-overview".into(),
            role: "overview".into(),
            title: "Workspaces".into(),
            subtitle: "Toggle scrollable overview".into(),
            command: LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}"),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-control".into(),
            role: "settings".into(),
            title: "Control Centre".into(),
            subtitle: "Dock style, wallpaper & sound".into(),
            command: LaunchTarget::Internal(ShellCmd::ToggleControl),
        },
    ];

    for win in wins {
        let win_role = role_for_app_id(&win.app_id);
        if let Some(existing) = items
            .iter_mut()
            .find(|e| e.app_id.eq_ignore_ascii_case(&win.app_id) || e.role == win_role)
        {
            existing.window_count += 1;
            if existing.window.is_none() || win.is_focused {
                existing.window = Some(win.id);
            }
            existing.is_focused |= win.is_focused;
        } else {
            items.push(AppEntry {
                window: Some(win.id),
                window_count: 1,
                is_focused: win.is_focused,
                app_id: win.app_id.clone(),
                role: win_role,
                title: pretty_app_name(&win.app_id),
                subtitle: win.title.clone(),
                command: LaunchTarget::Shell(win.app_id.clone()),
            });
        }
    }
    items
}

pub fn catalog_apps() -> Vec<AppEntry> {
    let term_bin = role_binary("terminal", "foot");
    let files_bin = role_binary("files", "solyra-files");
    let mut list = vec![
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: files_bin.clone(),
            role: "files".into(),
            title: "Files".into(),
            subtitle: "Browse local and mounted storage".into(),
            command: LaunchTarget::Shell("solyra-open files".into()),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: term_bin.clone(),
            role: "terminal".into(),
            title: "Terminal".into(),
            subtitle: "Command line shell".into(),
            command: LaunchTarget::Shell("solyra-open terminal".into()),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-overview".into(),
            role: "overview".into(),
            title: "Workspace Overview".into(),
            subtitle: "Zoom out all columns and workspaces".into(),
            command: LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}"),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-control".into(),
            role: "settings".into(),
            title: "Control Centre".into(),
            subtitle: "Dock style, wallpaper, sound and appearance".into(),
            command: LaunchTarget::Internal(ShellCmd::ToggleControl),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-agents".into(),
            role: "agent".into(),
            title: "Agent tasks".into(),
            subtitle: "Review agent progress and approval requests".into(),
            command: LaunchTarget::Internal(ShellCmd::ToggleAgents),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-workspace-7".into(),
            role: "overview".into(),
            title: "Go to workspace 7".into(),
            subtitle: "Plain workspace switch; not a sandbox".into(),
            command: LaunchTarget::Ipc(
                "{\"Action\":{\"FocusWorkspace\":{\"reference\":{\"Index\":7}}}}",
            ),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-wall".into(),
            role: "wallpaper".into(),
            title: "Ocean Wallpaper".into(),
            subtitle: "Switch the desktop backdrop to Ocean".into(),
            command: LaunchTarget::Internal(ShellCmd::SetWallpaper(WallpaperPreset::Ocean)),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-dock-mac".into(),
            role: "settings".into(),
            title: "Dock: macOS style".into(),
            subtitle: "Floating dock with running indicators".into(),
            command: LaunchTarget::Internal(ShellCmd::SetDock(DockStyle::Macos)),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-dock-win".into(),
            role: "settings".into(),
            title: "Dock: Taskbar style".into(),
            subtitle: "Full-width bar along the bottom edge".into(),
            command: LaunchTarget::Internal(ShellCmd::SetDock(DockStyle::Windows)),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-dock-sol".into(),
            role: "start".into(),
            title: "Dock: Solyra Hybrid".into(),
            subtitle: "Floating capsule with a start cell".into(),
            command: LaunchTarget::Internal(ShellCmd::SetDock(DockStyle::Solyra)),
        },
    ];

    // Also include additional installed `.desktop` entries without duplicating Terminal/Files.
    if let Ok(entries) = std::fs::read_dir("/usr/share/applications") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if stem.contains("foot")
                || stem == "thunar"
                || stem.contains("Xwayland")
                || stem.contains("portal")
                || stem.contains("geoclue")
                || stem.contains("panel-")
            {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            if raw.lines().any(|l| l.trim() == "NoDisplay=true") {
                continue;
            }
            let name = raw
                .lines()
                .find_map(|l| l.strip_prefix("Name="))
                .unwrap_or(&stem)
                .trim()
                .to_string();
            let comment = raw
                .lines()
                .find_map(|l| l.strip_prefix("Comment="))
                .unwrap_or("Installed application")
                .trim()
                .to_string();
            let Some(exec_raw) = raw.lines().find_map(|l| l.strip_prefix("Exec=")) else {
                continue;
            };
            let exec: String = exec_raw
                .split_whitespace()
                .filter(|tok| !tok.starts_with('%'))
                .collect::<Vec<_>>()
                .join(" ");
            if exec.is_empty() {
                continue;
            }
            list.push(AppEntry {
                window: None,
                window_count: 0,
                is_focused: false,
                app_id: stem.clone(),
                role: stem,
                title: name,
                subtitle: comment,
                command: LaunchTarget::Shell(exec),
            });
        }
    }
    list
}

pub fn load_app_icon<'a>(
    cache: &'a mut HashMap<String, Option<Icon>>,
    app_id: &str,
) -> Option<&'a Icon> {
    if !cache.contains_key(app_id) {
        let loaded = find_system_icon(app_id);
        cache.insert(app_id.to_string(), loaded);
    }
    cache.get(app_id).and_then(|v| v.as_ref())
}

fn find_system_icon(app_id: &str) -> Option<Icon> {
    const DIRS: &[&str] = &[
        "/usr/share/icons/hicolor/128x128/apps",
        "/usr/share/icons/hicolor/64x64/apps",
        "/usr/share/icons/hicolor/48x48/apps",
        "/usr/share/icons/Adwaita/64x64/apps",
        "/usr/share/icons/Adwaita/48x48/apps",
    ];
    let mut names = Vec::new();
    if let Ok(entry) = std::fs::read_to_string(format!("/usr/share/applications/{app_id}.desktop"))
    {
        for line in entry.lines() {
            if let Some(ic) = line.strip_prefix("Icon=") {
                names.push(ic.trim().to_string());
                break;
            }
        }
    }
    names.push(app_id.to_string());
    names.push(format!("org.xfce.{app_id}"));
    for name in names {
        for dir in DIRS {
            if let Ok(bytes) = std::fs::read(format!("{dir}/{name}.png")) {
                if let Some(icon) = decode_png(&bytes) {
                    return Some(icon);
                }
            }
        }
    }
    None
}

pub fn read_telemetry() -> Telemetry {
    let mut total_kib = 4 * 1024 * 1024u64;
    let mut avail_kib = 3 * 1024 * 1024u64;
    if let Ok(mem) = std::fs::read_to_string("/proc/meminfo") {
        for line in mem.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total_kib = rest
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(total_kib);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                avail_kib = rest
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(avail_kib);
            }
        }
    }
    let used_mib = total_kib.saturating_sub(avail_kib) / 1024;
    let total_mib = (total_kib / 1024).max(1);

    Telemetry {
        mem_used_mib: used_mib,
        mem_total_mib: total_mib,
        battery_pct: read_battery("/sys/class/power_supply"),
        network: read_network("/sys/class/net"),
        brightness_controllable: brightness_controllable(),
        volume_controllable: on_path("wpctl"),
    }
}

/// First real battery's charge. Mains adapters and USB supplies also expose
/// `capacity` on some machines, so only `type == Battery` counts.
pub fn read_battery(dir: &str) -> Option<u8> {
    let mut names: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    names.sort_by_key(|e| e.file_name());
    for entry in names {
        let path = entry.path();
        let kind = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(path.join("capacity")) {
            if let Ok(pct) = raw.trim().parse::<u8>() {
                return Some(pct.min(100));
            }
        }
    }
    None
}

/// Link state only: an interface with carrier is "connected". This does not
/// prove internet reachability and the UI must not say it does.
pub fn read_network(dir: &str) -> NetworkState {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return NetworkState::Offline;
    };
    let mut best = NetworkState::Offline;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "lo" {
            continue;
        }
        let up = std::fs::read_to_string(path.join("operstate"))
            .map(|s| s.trim() == "up")
            .unwrap_or(false);
        if !up {
            continue;
        }
        if path.join("wireless").exists() {
            if best == NetworkState::Offline {
                best = NetworkState::Wireless;
            }
        } else if path.join("device").exists() {
            // Physical wired NIC; virtual bridges and veths have no device.
            return NetworkState::Wired;
        }
    }
    best
}

fn brightness_controllable() -> bool {
    let has_backlight = std::fs::read_dir("/sys/class/backlight")
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    has_backlight && on_path("brightnessctl")
}

pub fn on_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solyra-state-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn supply(root: &PathBuf, name: &str, kind: &str, capacity: Option<&str>) {
        let d = root.join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("type"), format!("{kind}\n")).unwrap();
        if let Some(c) = capacity {
            fs::write(d.join("capacity"), format!("{c}\n")).unwrap();
        }
    }

    #[test]
    fn no_battery_is_none_not_a_placeholder() {
        let root = scratch("nobat");
        assert_eq!(read_battery(root.to_str().unwrap()), None);
        supply(&root, "AC", "Mains", Some("100"));
        assert_eq!(
            read_battery(root.to_str().unwrap()),
            None,
            "mains is not a battery"
        );
        assert_eq!(read_battery("/definitely/not/here"), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn battery_reads_the_battery_and_clamps() {
        let root = scratch("bat");
        supply(&root, "AC", "Mains", Some("100"));
        supply(&root, "BAT0", "Battery", Some("57"));
        assert_eq!(read_battery(root.to_str().unwrap()), Some(57));
        supply(&root, "BAT0", "Battery", Some("250"));
        assert_eq!(read_battery(root.to_str().unwrap()), Some(100));
        let _ = fs::remove_dir_all(&root);
    }

    fn iface(root: &PathBuf, name: &str, state: &str, wireless: bool, physical: bool) {
        let d = root.join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("operstate"), format!("{state}\n")).unwrap();
        if wireless {
            fs::create_dir_all(d.join("wireless")).unwrap();
        }
        if physical {
            fs::create_dir_all(d.join("device")).unwrap();
        }
    }

    #[test]
    fn network_state_from_link_state() {
        let root = scratch("net");
        let path = root.to_str().unwrap();
        assert_eq!(read_network(path), NetworkState::Offline);
        iface(&root, "lo", "up", false, false);
        iface(&root, "veth0", "up", false, false);
        assert_eq!(
            read_network(path),
            NetworkState::Offline,
            "loopback/virtual do not count"
        );
        iface(&root, "wlan0", "down", true, true);
        assert_eq!(read_network(path), NetworkState::Offline);
        iface(&root, "wlan0", "up", true, true);
        assert_eq!(read_network(path), NetworkState::Wireless);
        iface(&root, "eth0", "up", false, true);
        assert_eq!(read_network(path), NetworkState::Wired);
        assert_eq!(read_network("/definitely/not/here"), NetworkState::Offline);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wallpaper_validation_checks_type_and_existence() {
        let root = scratch("wall");
        let png = root.join("ok.png");
        fs::write(&png, [0x89, b'P', b'N', b'G', 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        let jpg = root.join("ok.jpg");
        fs::write(&jpg, [0xff, 0xd8, 0xff, 0xe0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        let txt = root.join("notes.png");
        fs::write(&txt, b"definitely not an image").unwrap();
        assert!(validate_wallpaper(png.to_str().unwrap()).is_ok());
        assert!(validate_wallpaper(jpg.to_str().unwrap()).is_ok());
        assert!(validate_wallpaper(txt.to_str().unwrap())
            .unwrap_err()
            .contains("not a PNG or JPEG"));
        assert!(validate_wallpaper(root.join("missing.png").to_str().unwrap()).is_err());
        assert!(validate_wallpaper(root.to_str().unwrap())
            .unwrap_err()
            .contains("not a file"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wallpaper_file_command_parses_paths_with_spaces() {
        match parse_cmd_line("wallpaper-file fit /home/u/My Pictures/a b.png") {
            Some(ShellCmd::SetWallpaperFile { mode, path }) => {
                assert_eq!(mode, WallpaperMode::Fit);
                assert_eq!(path, "/home/u/My Pictures/a b.png");
            }
            other => panic!("{other:?}"),
        }
        assert!(parse_cmd_line("wallpaper-file weird /x.png").is_none());
        assert!(parse_cmd_line("wallpaper-file fill").is_none());
    }

    #[test]
    fn cmd_lines_parse() {
        assert!(matches!(
            parse_cmd_line("toggle-agents"),
            Some(ShellCmd::ToggleAgents)
        ));
        assert!(matches!(
            parse_cmd_line("agents"),
            Some(ShellCmd::ToggleAgents)
        ));
        assert!(
            matches!(parse_cmd_line("notify hi there"), Some(ShellCmd::Notify(_, t)) if t == "hi there")
        );
        assert!(parse_cmd_line("rm -rf /").is_none());
    }

    #[test]
    fn agent_env_names_round_trip() {
        assert_eq!(parse_agent_env("cloud"), Some(0));
        assert_eq!(parse_agent_env("Local"), Some(1));
        assert_eq!(parse_agent_env("worktree"), Some(2));
        assert_eq!(parse_agent_env("nope"), None);
        assert_eq!(agent_env_name(0), "cloud");
        assert_eq!(agent_env_name(2), "worktree");
        assert_eq!(agent_env_name(9), "local");
        assert_eq!(ShellPrefs::default().agent_env, 1);
    }

    #[test]
    fn catalog_makes_no_isolation_claims() {
        for app in catalog_apps() {
            let text = format!("{} {}", app.title, app.subtitle).to_ascii_lowercase();
            assert!(!text.contains("isolated"), "{text}");
        }
    }
}

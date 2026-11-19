//! Compositor state, desktop app discovery, live IPC command socket, and
//! persisted shell preferences.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::canvas::{decode_png, Icon, Rgba};

pub const ISLAND_TTL_SECS: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockStyle {
    Solyra,
    Macos,
    Windows,
}

impl DockStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solyra => "solyra",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Solyra => "Solyra",
            Self::Macos => "macOS",
            Self::Windows => "Windows",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "solyra" | "hybrid" => Some(Self::Solyra),
            "macos" | "mac" => Some(Self::Macos),
            "windows" | "win" | "taskbar" => Some(Self::Windows),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperPreset {
    Violet,
    Ocean,
    Sunset,
    Forest,
    Graphite,
}

impl WallpaperPreset {
    pub const ALL: [Self; 5] = [
        Self::Violet,
        Self::Ocean,
        Self::Sunset,
        Self::Forest,
        Self::Graphite,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Violet => "violet",
            Self::Ocean => "ocean",
            Self::Sunset => "sunset",
            Self::Forest => "forest",
            Self::Graphite => "graphite",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Violet => "Violet",
            Self::Ocean => "Ocean",
            Self::Sunset => "Sunset",
            Self::Forest => "Forest",
            Self::Graphite => "Graphite",
        }
    }

    pub fn swatch(self) -> Rgba {
        match self {
            Self::Violet => Rgba::rgb(0xa8, 0x55, 0xf7),
            Self::Ocean => Rgba::rgb(0x0e, 0xa5, 0xe9),
            Self::Sunset => Rgba::rgb(0xf9, 0x73, 0x16),
            Self::Forest => Rgba::rgb(0x10, 0xb9, 0x81),
            Self::Graphite => Rgba::rgb(0x64, 0x74, 0x8b),
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
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

#[derive(Clone, Debug)]
pub enum ShellCmd {
    Notify(String),
    ToggleLauncher,
    ToggleControl,
    ToggleIsland,
    CloseOverlays,
    SetDock(DockStyle),
    SetWallpaper(WallpaperPreset),
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
    pub battery_pct: u8,
}

pub struct ShellPrefs {
    pub dock_style: DockStyle,
    pub wallpaper: WallpaperPreset,
    pub wifi_on: bool,
    pub bt_on: bool,
    pub dnd_on: bool,
    pub agent_ready: bool,
    pub volume: u8,
    pub brightness: u8,
}

impl Default for ShellPrefs {
    fn default() -> Self {
        Self {
            dock_style: DockStyle::Solyra,
            wallpaper: WallpaperPreset::Violet,
            wifi_on: true,
            bt_on: true,
            dnd_on: false,
            agent_ready: true,
            volume: 76,
            brightness: 84,
        }
    }
}

impl ShellPrefs {
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
                "volume" => {
                    if let Ok(n) = v.trim().parse::<u8>() {
                        prefs.volume = n.min(100);
                    }
                }
                "brightness" => {
                    if let Ok(n) = v.trim().parse::<u8>() {
                        prefs.brightness = n.clamp(15, 100);
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
            let _ = std::fs::create_dir_all(parent);
        }
        let body = format!(
            "dock_style={}\nwallpaper={}\nvolume={}\nbrightness={}\n",
            self.dock_style.as_str(),
            self.wallpaper.as_str(),
            self.volume,
            self.brightness,
        );
        let _ = std::fs::write(path, body);
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
        return Some(ShellCmd::Notify(rest.trim().to_string()));
    }
    if let Some(rest) = s.strip_prefix("dock ") {
        return DockStyle::parse(rest).map(ShellCmd::SetDock);
    }
    if let Some(rest) = s.strip_prefix("wallpaper ") {
        return WallpaperPreset::parse(rest).map(ShellCmd::SetWallpaper);
    }
    match s {
        "toggle-launcher" | "launcher" => Some(ShellCmd::ToggleLauncher),
        "toggle-control" | "control" => Some(ShellCmd::ToggleControl),
        "toggle-island" | "island" => Some(ShellCmd::ToggleIsland),
        "close" => Some(ShellCmd::CloseOverlays),
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

pub fn apply_wallpaper(preset: WallpaperPreset) {
    let path = preset.path();
    let cmd = format!(
        "old=$(pidof swaybg 2>/dev/null || true); \
         swaybg -i '{}' -m fill >/dev/null 2>&1 & \
         sleep 0.2; \
         [ -n \"$old\" ] && kill $old >/dev/null 2>&1 || true",
        path
    );
    let _ = std::process::Command::new("sh").arg("-c").arg(cmd).spawn();
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
    stream.set_read_timeout(Some(Duration::from_millis(350))).ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    Some(reply)
}

pub fn send_ipc(request: &str) {
    let _ = ipc(request);
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
            let active = ws.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
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
            let workspace_id = win.get("workspace_id").and_then(|v| v.as_u64()).unwrap_or(1);
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

pub fn dock_entries(wins: &[WindowInfo]) -> Vec<AppEntry> {
    let term_bin = role_binary("terminal", "foot");
    let files_bin = role_binary("files", "thunar");

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
            command: LaunchTarget::Shell(files_bin),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: term_bin.clone(),
            role: "terminal".into(),
            title: "Terminal".into(),
            subtitle: "Command line shell".into(),
            command: LaunchTarget::Shell(term_bin),
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
    let files_bin = role_binary("files", "thunar");
    let mut list = vec![
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: files_bin.clone(),
            role: "files".into(),
            title: "Files".into(),
            subtitle: "Thunar File Manager · Browse local & mounted storage".into(),
            command: LaunchTarget::Shell(files_bin),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: term_bin.clone(),
            role: "terminal".into(),
            title: "Terminal".into(),
            subtitle: "Foot Wayland Terminal · Fast GPU/shm shell".into(),
            command: LaunchTarget::Shell(term_bin),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-overview".into(),
            role: "overview".into(),
            title: "Workspace Overview".into(),
            subtitle: "Zoom out all columns and workspaces (Super+Tab)".into(),
            command: LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}"),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-control".into(),
            role: "settings".into(),
            title: "Control Centre".into(),
            subtitle: "Switch dock variant, wallpaper, audio & display".into(),
            command: LaunchTarget::Internal(ShellCmd::ToggleControl),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-agent".into(),
            role: "agent".into(),
            title: "Agent Space".into(),
            subtitle: "Switch to isolated Agent Workspace 7".into(),
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
            subtitle: "Switch desktop backdrop to deep Ocean aurora".into(),
            command: LaunchTarget::Internal(ShellCmd::SetWallpaper(WallpaperPreset::Ocean)),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-dock-mac".into(),
            role: "settings".into(),
            title: "Dock: macOS Sequoia Style".into(),
            subtitle: "Floating glass dock with running dots & Trash".into(),
            command: LaunchTarget::Internal(ShellCmd::SetDock(DockStyle::Macos)),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-dock-win".into(),
            role: "settings".into(),
            title: "Dock: Windows 11 Taskbar".into(),
            subtitle: "Full-width bottom bar with search pill & tray".into(),
            command: LaunchTarget::Internal(ShellCmd::SetDock(DockStyle::Windows)),
        },
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: "solyra-dock-sol".into(),
            role: "start".into(),
            title: "Dock: Solyra Hybrid".into(),
            subtitle: "Floating capsule with Start mark & Agent shelf".into(),
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

    let mut battery_pct = 96u8;
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let cap = entry.path().join("capacity");
            if let Ok(raw) = std::fs::read_to_string(cap) {
                if let Ok(pct) = raw.trim().parse::<u8>() {
                    battery_pct = pct.min(100);
                    break;
                }
            }
        }
    }
    Telemetry {
        mem_used_mib: used_mib,
        mem_total_mib: total_mib,
        battery_pct,
    }
}

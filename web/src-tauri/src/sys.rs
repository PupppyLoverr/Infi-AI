//! System data for the web shell. Everything shown is read from the real
//! system at the moment it is asked for; nothing is invented. Missing tools
//! or hardware give empty / `None` results the UI shows as such.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

/// Run a shell command detached from us.
pub fn launch(cmd: &str) {
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!("{cmd} >/dev/null 2>&1 &"))
        .spawn();
}

// ---------------------------------------------------------------- apps

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub exec: String,
    /// Solyra role (files, browser, terminal, ...) when it has one; the
    /// frontend uses it to pick a bundled icon.
    pub role: String,
    pub icon: String,
}

/// The role a desktop id / binary plays (matches the panel's icons.rs).
pub fn role_for(key: &str) -> String {
    let k = key.to_ascii_lowercase();
    let has = |n: &[&str]| n.iter().any(|x| k.contains(x));
    let r = if has(&["solyra-files", "thunar", "nautilus", "dolphin", "files"]) {
        "files"
    } else if has(&["firefox", "chrom", "browser", "epiphany", "solyra-browse"]) {
        "browser"
    } else if has(&["term", "foot", "kitty", "alacritty", "konsole"]) {
        "terminal"
    } else if has(&["settings", "control-center"]) {
        "settings"
    } else if has(&["mousepad", "gedit", "text-editor", "kate", "editor"]) {
        "editor"
    } else if has(&["ristretto", "eog", "loupe", "viewer", "photo"]) {
        "viewer"
    } else if has(&["calc", "galculator"]) {
        "calculator"
    } else if has(&["music", "rhythmbox"]) {
        "music"
    } else if has(&["mail", "thunderbird"]) {
        "mail"
    } else {
        ""
    };
    r.to_string()
}

fn parse_desktop(path: &Path) -> Option<App> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut comment, mut exec, mut icon) = (None, String::new(), None, String::new());
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            name.get_or_insert(v.to_string());
        } else if let Some(v) = line.strip_prefix("Comment=") {
            if comment.is_empty() {
                comment = v.to_string();
            }
        } else if let Some(v) = line.strip_prefix("Exec=") {
            exec.get_or_insert(v.to_string());
        } else if let Some(v) = line.strip_prefix("Icon=") {
            icon = v.to_string();
        } else if line == "NoDisplay=true" || line == "Hidden=true" {
            return None;
        } else if let Some(v) = line.strip_prefix("Type=") {
            if v != "Application" {
                return None;
            }
        }
    }
    let exec = exec?
        .split_whitespace()
        .filter(|t| !t.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ");
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let role = role_for(&format!("{id} {exec}"));
    Some(App { id, name: name?, comment, exec, role, icon })
}

pub fn apps() -> Vec<App> {
    let mut dirs = vec![data_home().join("applications")];
    let xdg = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(xdg.split(':').map(|d| Path::new(d).join("applications")));
    let mut out: Vec<App> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "desktop") {
                if let Some(a) = parse_desktop(&p) {
                    if !out.iter().any(|o| o.id == a.id) {
                        out.push(a);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

// ---------------------------------------------------------------- windows (niri IPC)

#[derive(Serialize, Clone, Debug)]
pub struct Win {
    pub id: u64,
    pub app_id: String,
    pub title: String,
    pub focused: bool,
}

/// A window's dock role. solyra-web's windows share one app id, so their
/// title (Files, Terminal) decides.
pub fn window_role(w: &Win) -> String {
    let id = w.app_id.to_ascii_lowercase();
    if id.contains("solyra") && id.contains("web") {
        return role_for(&w.title);
    }
    role_for(&w.app_id)
}

pub fn windows() -> Vec<Win> {
    let Some(raw) = run("niri", &["msg", "-j", "windows"]) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { return Vec::new() };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|w| {
                    Some(Win {
                        id: w.get("id")?.as_u64()?,
                        app_id: w.get("app_id").and_then(|x| x.as_str()).unwrap_or("").into(),
                        title: w.get("title").and_then(|x| x.as_str()).unwrap_or("").into(),
                        focused: w.get("is_focused").and_then(|x| x.as_bool()).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn focus_window(id: u64) {
    let _ = Command::new("niri")
        .args(["msg", "action", "focus-window", "--id", &id.to_string()])
        .status();
}

// ---------------------------------------------------------------- dock

#[derive(Serialize, Clone, Debug)]
pub struct Pin {
    pub id: String,
    pub name: String,
    pub role: String,
    pub cmd: String,
    pub running: usize,
    pub focused: bool,
    pub window: Option<u64>,
}

/// The pinned apps (pearOS order), each only if it is installed, with the
/// windows that belong to it; then running apps that are not pinned.
pub fn dock_pins() -> Vec<Pin> {
    // (role, name, binary that must be installed, launch command). The web
    // apps (Files, Terminal) come first when solyra-web is installed.
    let web = on_path("solyra-web");
    let candidates: [(&str, &str, &str, &str); 7] = [
        if web { ("files", "Files", "solyra-web", "solyra-web files") } else { ("files", "Files", "solyra-files", "solyra-open files") },
        ("browser", "Browser", "firefox", "solyra-open browser"),
        if web { ("terminal", "Terminal", "solyra-web", "solyra-web terminal") } else { ("terminal", "Terminal", "foot", "solyra-open terminal") },
        ("editor", "Text Editor", "mousepad", "solyra-open editor"),
        ("viewer", "Image Viewer", "ristretto", "solyra-open viewer"),
        ("calculator", "Calculator", "galculator", "galculator"),
        ("settings", "Settings", "solyra-settings", "solyra-settings"),
    ];
    let wins = windows();
    let mut used = vec![false; wins.len()];
    let mut pins = Vec::new();
    for (role, name, bin, cmd) in candidates {
        if !on_path(bin) {
            continue;
        }
        let mine: Vec<usize> = wins
            .iter()
            .enumerate()
            .filter(|(_, w)| window_role(w) == role)
            .map(|(i, _)| i)
            .collect();
        for &i in &mine {
            used[i] = true;
        }
        pins.push(Pin {
            id: format!("{bin}:{role}"),
            name: name.into(),
            role: role.into(),
            cmd: cmd.into(),
            running: mine.len(),
            focused: mine.iter().any(|&i| wins[i].focused),
            window: mine.first().map(|&i| wins[i].id),
        });
    }
    for (i, w) in wins.iter().enumerate() {
        if used[i] || w.app_id.is_empty() || pins.iter().any(|p| p.id == w.app_id) {
            continue;
        }
        pins.push(Pin {
            id: w.app_id.clone(),
            name: if w.title.is_empty() { w.app_id.clone() } else { w.title.clone() },
            role: window_role(w),
            cmd: String::new(),
            running: 1,
            focused: w.focused,
            window: Some(w.id),
        });
    }
    pins
}

// ---------------------------------------------------------------- system status

#[derive(Serialize, Clone, Debug, Default)]
pub struct Status {
    pub battery: Option<u8>,
    pub charging: bool,
    pub volume: Option<u8>,
    pub muted: bool,
    pub brightness: Option<u8>,
    pub wifi: Option<String>,
    pub online: bool,
}

pub fn status() -> Status {
    let mut s = Status::default();
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            if std::fs::read_to_string(p.join("type")).ok().as_deref().map(str::trim) == Some("Battery") {
                s.battery = std::fs::read_to_string(p.join("capacity")).ok().and_then(|v| v.trim().parse().ok());
                s.charging = matches!(
                    std::fs::read_to_string(p.join("status")).ok().as_deref().map(str::trim),
                    Some("Charging" | "Full")
                );
                break;
            }
        }
    }
    if let Some(out) = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]) {
        if let Some(v) = out.split_whitespace().nth(1).and_then(|v| v.parse::<f32>().ok()) {
            s.volume = Some((v * 100.0).round().clamp(0.0, 100.0) as u8);
            s.muted = out.contains("MUTED");
        }
    }
    if let Some(out) = run("brightnessctl", &["-m"]) {
        s.brightness = out.split(',').nth(3).and_then(|p| p.trim_end_matches('%').parse().ok());
    }
    if let Some(out) = run("nmcli", &["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]) {
        s.wifi = out
            .lines()
            .find_map(|l| l.strip_prefix("yes:").map(str::to_string))
            .filter(|x| !x.is_empty());
    }
    s.online = run("nmcli", &["-t", "-f", "STATE", "general"])
        .map(|o| o.trim().starts_with("connected"))
        .unwrap_or(false);
    s
}

pub fn set_volume(pct: u8) {
    let v = format!("{:.2}", pct.min(100) as f32 / 100.0);
    let _ = Command::new("wpctl").args(["set-volume", "@DEFAULT_AUDIO_SINK@", &v]).status();
}

pub fn set_brightness(pct: u8) {
    let _ = Command::new("brightnessctl").args(["-q", "set", &format!("{}%", pct.clamp(1, 100))]).status();
}

// ---------------------------------------------------------------- clipboard + shelf (written by solyra-panel)

#[derive(Serialize, Clone, Debug)]
pub struct Clip {
    pub id: u64,
    pub text: String,
    pub kind: String,
    pub category: String,
    pub pinned: bool,
}

fn classify(text: &str) -> &'static str {
    let t = text.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        "link"
    } else if t.starts_with('#') && (t.len() == 7 || t.len() == 4) && t[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        "color"
    } else if t.starts_with('/') && Path::new(t).exists() {
        "file"
    } else {
        "text"
    }
}

pub fn clipboard() -> Vec<Clip> {
    let Ok(raw) = std::fs::read_to_string(data_home().join("solyra/clipboard.json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { return Vec::new() };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let text = c.get("text")?.as_str()?.to_string();
                    Some(Clip {
                        id: c.get("id").and_then(|x| x.as_u64()).unwrap_or(0),
                        kind: classify(&text).into(),
                        category: c.get("category").and_then(|x| x.as_str()).unwrap_or("all").into(),
                        pinned: c.get("pinned").and_then(|x| x.as_bool()).unwrap_or(false),
                        text,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn copy_text(text: &str) {
    use std::io::Write;
    if let Ok(mut child) = Command::new("wl-copy").stdin(std::process::Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ShelfItem {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

pub fn shelf() -> Vec<ShelfItem> {
    crate::shelf::load()
        .into_iter()
        .map(|p| ShelfItem {
            name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            is_dir: p.is_dir(),
            path: p.to_string_lossy().into_owned(),
        })
        .collect()
}

pub fn shelf_add(path: &str) {
    let mut items = crate::shelf::load();
    crate::shelf::add(&mut items, vec![PathBuf::from(path)]);
    crate::shelf::save(&items);
}

pub fn shelf_remove(path: &str) {
    let mut items = crate::shelf::load();
    items.retain(|p| p.to_string_lossy() != path);
    crate::shelf::save(&items);
}

// ---------------------------------------------------------------- files

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
    pub kind: String,
}

fn kind_of(p: &Path, is_dir: bool) -> String {
    if is_dir {
        return "Folder".into();
    }
    match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") => "Image",
        Some("mp4" | "mkv" | "mov" | "webm") => "Video",
        Some("mp3" | "flac" | "ogg" | "wav" | "m4a") => "Audio",
        Some("pdf") => "PDF document",
        Some("md" | "txt" | "log") => "Text document",
        Some("rs" | "ts" | "tsx" | "js" | "py" | "c" | "h" | "go" | "toml" | "json" | "sh") => "Source code",
        Some("zip" | "tar" | "gz" | "xz" | "zst") => "Archive",
        _ => "Document",
    }
    .into()
}

pub fn expand(path: &str) -> PathBuf {
    if path == "~" {
        home()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(path)
    }
}

pub fn list_dir(path: &str, hidden: bool) -> Result<Vec<Entry>, String> {
    let dir = expand(path);
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                return None;
            }
            let md = e.metadata().ok()?;
            let p = e.path();
            Some(Entry {
                kind: kind_of(&p, md.is_dir()),
                is_dir: md.is_dir(),
                size: md.len(),
                modified: md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                path: p.to_string_lossy().into_owned(),
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

pub fn open_path(path: &str) {
    let p = expand(path);
    let _ = Command::new("xdg-open").arg(&p).spawn();
}

pub fn home_dir() -> String {
    home().to_string_lossy().into_owned()
}

/// Files under the home folder whose names contain `q` (case-insensitive),
/// breadth-first, at most `max`, at most 4 levels deep, skipping hidden.
pub fn find_files(q: &str, max: usize) -> Vec<Entry> {
    let q = q.trim().to_lowercase();
    if q.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::from([(home(), 0usize)]);
    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            visited += 1;
            if visited > 20_000 {
                return out;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let p = e.path();
            let is_dir = p.is_dir();
            if name.to_lowercase().contains(&q) {
                out.push(Entry {
                    kind: kind_of(&p, is_dir),
                    is_dir,
                    size: 0,
                    modified: 0,
                    path: p.to_string_lossy().into_owned(),
                    name,
                });
                if out.len() >= max {
                    return out;
                }
            }
            if is_dir && depth < 4 {
                queue.push_back((p, depth + 1));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_match_the_panel() {
        assert_eq!(role_for("org.mozilla.firefox"), "browser");
        assert_eq!(role_for("foot"), "terminal");
        assert_eq!(role_for("solyra-files"), "files");
        assert_eq!(role_for("galculator"), "calculator");
        assert_eq!(role_for("zzz"), "");
    }

    #[test]
    fn desktop_entries_parse() {
        let dir = std::env::temp_dir().join(format!("solyra-web-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("org.example.Edit.desktop");
        std::fs::write(
            &p,
            "[Desktop Entry]\nType=Application\nName=Edit\nComment=Write\nExec=mousepad %F\nIcon=x\n[Desktop Action new]\nName=Other\n",
        )
        .unwrap();
        let a = parse_desktop(&p).unwrap();
        assert_eq!((a.name.as_str(), a.exec.as_str(), a.role.as_str()), ("Edit", "mousepad", "editor"));
        std::fs::write(&p, "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n").unwrap();
        assert!(parse_desktop(&p).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clipboard_kinds() {
        assert_eq!(classify("https://example.com"), "link");
        assert_eq!(classify("#FF9F43"), "color");
        assert_eq!(classify("hello"), "text");
    }
}

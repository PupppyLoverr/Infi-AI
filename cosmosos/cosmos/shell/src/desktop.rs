//! Real .desktop entries: scan XDG application dirs, parse, launch.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Debug, Clone)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub terminal: bool,
    pub comment: String,
    /// System action rather than an app (logout, shutdown, ...).
    pub action: Option<SystemAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAction {
    Lock,
    Logout,
    Reboot,
    Shutdown,
}

impl AppEntry {
    fn action_entry(id: &str, name: &str, icon: &str, action: SystemAction) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            exec: String::new(),
            icon: icon.to_string(),
            terminal: false,
            comment: String::new(),
            action: Some(action),
        }
    }
}

/// Scan XDG applications dirs for real .desktop files.
pub fn scan_apps() -> Vec<AppEntry> {
    let mut dirs = vec![PathBuf::from("/usr/share/applications")];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    if let Ok(data) = std::env::var("XDG_DATA_DIRS") {
        for d in data.split(':') {
            let p = PathBuf::from(d).join("applications");
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }

    let mut apps: HashMap<String, AppEntry> = HashMap::new();
    for dir in dirs {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(app) = parse_desktop(&path) else {
                continue;
            };
            // First found wins: user-local entries shadow system ones because
            // the local dir is listed last in `dirs` only if we want it to
            // override — here later entries override, matching XDG priority.
            apps.insert(app.id.clone(), app);
        }
    }
    let mut out: Vec<_> = apps.into_values().collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn parse_desktop(path: &PathBuf) -> Option<AppEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;
    let mut icon = String::new();
    let mut terminal = false;
    let mut comment = String::new();
    let mut in_desktop_entry = false;
    let mut hidden = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "Name" => name = Some(value.to_string()),
            "Exec" => exec = Some(value.to_string()),
            "Icon" => icon = value.to_string(),
            "Terminal" => terminal = value.eq_ignore_ascii_case("true"),
            "Comment" => comment = value.to_string(),
            "NoDisplay" | "Hidden" if value.eq_ignore_ascii_case("true") => hidden = true,
            _ => {}
        }
    }
    if hidden {
        return None;
    }
    Some(AppEntry {
        id: path.file_stem()?.to_str()?.to_string(),
        name: name?,
        exec: exec?,
        icon,
        terminal,
        comment,
        action: None,
    })
}

/// Strip .desktop Exec field codes (%f %F %u %U %i %c %k %d %D %n %N %v %m).
fn clean_exec(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// Launch the entry for real. Returns an error string for the UI if it fails.
pub fn launch(entry: &AppEntry) -> Result<(), String> {
    if let Some(action) = entry.action {
        return run_action(action);
    }
    let cmd = clean_exec(&entry.exec);
    if cmd.is_empty() {
        return Err("empty Exec".into());
    }
    let spawn = |mut c: Command| -> Result<(), String> {
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("spawn failed: {e}"))
    };
    if entry.terminal {
        let term =
            std::env::var("COSMOS_TERMINAL").unwrap_or_else(|_| "cosmos-terminal".to_string());
        let mut c = Command::new(term);
        c.arg("-e").arg("sh").arg("-c").arg(&cmd);
        spawn(c)
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&cmd);
        spawn(c)
    }
}

fn run_action(action: SystemAction) -> Result<(), String> {
    match action {
        // Session end goes through the compositor so it can clean up.
        SystemAction::Logout => {
            let _ = cosmos_ipc::write_message(
                &mut match std::os::unix::net::UnixStream::connect(cosmos_ipc::socket_path()) {
                    Ok(s) => s,
                    Err(e) => return Err(format!("ipc: {e}")),
                },
                &cosmos_ipc::Request::QuitSession,
            );
            Ok(())
        }
        SystemAction::Reboot => dbus_power("Reboot"),
        SystemAction::Shutdown => dbus_power("PowerOff"),
        SystemAction::Lock => dbus_power("Lock").or_else(|_| dbus_lock_session()),
    }
}

/// logind power calls — the real path on systemd systems.
fn dbus_power(method: &str) -> Result<(), String> {
    let conn = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
    conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        Some("org.freedesktop.login1.Manager"),
        method,
        &(),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn dbus_lock_session() -> Result<(), String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1/session/auto",
        Some("org.freedesktop.login1.Session"),
        "Lock",
        &(),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Built-in entries always shown at the bottom of the launcher.
pub fn system_entries() -> Vec<AppEntry> {
    vec![
        AppEntry::action_entry("sys.lock", "Lock", "system-lock-screen", SystemAction::Lock),
        AppEntry::action_entry(
            "sys.logout",
            "Log out",
            "system-log-out",
            SystemAction::Logout,
        ),
        AppEntry::action_entry(
            "sys.reboot",
            "Reboot",
            "system-reboot",
            SystemAction::Reboot,
        ),
        AppEntry::action_entry(
            "sys.shutdown",
            "Shut down",
            "system-shutdown",
            SystemAction::Shutdown,
        ),
    ]
}

//! Real system state: clock, battery, network, audio.
//!
//! Probes run on a worker thread; results feed the panel through a channel.

use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

use calloop::channel::Sender;

#[derive(Debug, Clone, Default)]
pub struct SysInfo {
    pub clock: String,
    pub date: String,
    pub battery: Option<Battery>,
    pub network: NetworkState,
    pub volume: Option<Volume>,
}

#[derive(Debug, Clone)]
pub struct Battery {
    /// 0..=100
    pub percent: u8,
    pub charging: bool,
    pub present: bool,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkState {
    /// Short label: "Wired", "Wi-Fi", "Offline", or interface/SSID name.
    pub label: String,
    pub online: bool,
}

#[derive(Debug, Clone)]
pub struct Volume {
    /// 0.0..=1.0 (can exceed 1.0 on some PipeWire setups — clamp at draw).
    pub level: f32,
    pub muted: bool,
}

/// Spawn the probe worker; pushes SysInfo snapshots on `tx` every few seconds.
pub fn start(tx: Sender<SysInfo>) {
    std::thread::spawn(move || loop {
        let _ = tx.send(poll());
        std::thread::sleep(Duration::from_secs(3));
    });
}

fn poll() -> SysInfo {
    SysInfo {
        clock: clock(),
        date: date_str(),
        battery: battery(),
        network: network(),
        volume: volume(),
    }
}

// ---------------------------------------------------------------- clock

fn clock() -> String {
    // v3 menubar form: "Thu 8 Oct 22:14" — weekday, day, short month,
    // then the time. Weekday comes from days since epoch (1970-01-01
    // was a Thursday), so no extra libc call is needed.
    libc_tm()
        .map(|(h, m, d, mo, _)| {
            const WDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let days = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() / 86_400)
                .unwrap_or(0);
            let wday = WDAYS[(days % 7) as usize];
            format!(
                "{} {} {} {:02}:{:02}",
                wday,
                d,
                MONTHS[mo as usize % 12],
                h,
                m
            )
        })
        .unwrap_or_default()
}

fn date_str() -> String {
    libc_tm()
        .map(|(_, _, d, mo, y)| format!("{y}-{:02}-{d:02}", mo + 1))
        .unwrap_or_default()
}

/// hour, min, mday, mon0, year1900 — via libc localtime (real timezone).
fn libc_tm() -> Option<(i32, i32, i32, i32, i32)> {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs() as libc::time_t;
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return None;
        }
        Some((
            tm.tm_hour,
            tm.tm_min,
            tm.tm_mday,
            tm.tm_mon,
            tm.tm_year + 1900,
        ))
    }
}

// -------------------------------------------------------------- battery

fn battery() -> Option<Battery> {
    let dir = PathBuf::from("/sys/class/power_supply");
    let read = dir.read_dir().ok()?;
    for entry in read.flatten() {
        let p = entry.path();
        if std::fs::read_to_string(p.join("type"))
            .map(|t| t.trim() == "Battery")
            .unwrap_or(false)
        {
            let percent = std::fs::read_to_string(p.join("capacity"))
                .ok()
                .and_then(|v| v.trim().parse::<u8>().ok())
                .unwrap_or(0);
            let status = std::fs::read_to_string(p.join("status")).unwrap_or_default();
            let charging = matches!(status.trim(), "Charging" | "Full");
            return Some(Battery {
                percent,
                charging,
                present: true,
            });
        }
    }
    None
}

// -------------------------------------------------------------- network

/// NetworkManager over D-Bus; falls back to /sys/class/net + default route.
fn network() -> NetworkState {
    if let Some(state) = nm_state() {
        return state;
    }
    sysclass_network()
}

fn nm_state() -> Option<NetworkState> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.NetworkManager"),
            "/org/freedesktop/NetworkManager",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.NetworkManager", "State"),
        )
        .ok()?;
    let body = reply.body();
    let v = body.deserialize::<zbus::zvariant::Value>().ok()?;
    let state: u32 = match v {
        zbus::zvariant::Value::U32(s) => s,
        _ => return None,
    };
    // NM state: 70=connected(global), 60=connected(site), 50=connected(local),
    // anything below is degraded/offline.
    let online = state >= 50;
    let label = if online {
        primary_connection_name(&conn).unwrap_or_else(|| "Online".to_string())
    } else {
        "Offline".to_string()
    };
    Some(NetworkState { label, online })
}

fn primary_connection_name(conn: &zbus::blocking::Connection) -> Option<String> {
    // PrimaryConnection object path → that connection's Id.
    let reply = conn
        .call_method(
            Some("org.freedesktop.NetworkManager"),
            "/org/freedesktop/NetworkManager",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.NetworkManager", "PrimaryConnection"),
        )
        .ok()?;
    let body = reply.body();
    let v = body.deserialize::<zbus::zvariant::Value>().ok()?;
    let path = match v {
        zbus::zvariant::Value::ObjectPath(p) => p.to_string(),
        _ => return None,
    };
    if path == "/" {
        return None;
    }
    let reply = conn
        .call_method(
            Some("org.freedesktop.NetworkManager"),
            &*path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.NetworkManager.Connection.Active", "Id"),
        )
        .ok()?;
    let body = reply.body();
    match body.deserialize::<zbus::zvariant::Value>().ok()? {
        zbus::zvariant::Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn sysclass_network() -> NetworkState {
    // No NM: check /sys/class/net for a live non-lo link and a default route.
    let mut label = "Offline".to_string();
    let mut online = false;
    if let Ok(read) = std::fs::read_dir("/sys/class/net") {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                continue;
            }
            let up = std::fs::read_to_string(entry.path().join("operstate"))
                .map(|s| s.trim() == "up")
                .unwrap_or(false);
            if up {
                label = if name.starts_with('w') {
                    "Wi-Fi".to_string()
                } else {
                    name.clone()
                };
                online = true;
                break;
            }
        }
    }
    if online {
        online = std::fs::read_to_string("/proc/net/route")
            .map(|r| {
                r.lines()
                    .skip(1)
                    .any(|l| l.split_whitespace().nth(1) == Some("00000000"))
            })
            .unwrap_or(false)
            || std::fs::read_to_string("/proc/net/ipv6_route")
                .map(|r| {
                    r.lines()
                        .any(|l| l.contains("00000000000000000000000000000000"))
                })
                .unwrap_or(false);
        if !online {
            label = "Link".to_string();
        }
    }
    NetworkState { label, online }
}

// ---------------------------------------------------------------- audio

/// PipeWire default sink volume via wpctl (ships with pipewire).
fn volume() -> Option<Volume> {
    let out = std::process::Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // e.g. "Volume: 0.35" or "Volume: 0.35 [MUTED]"
    let mut level = None;
    for tok in text.split_whitespace() {
        if let Ok(v) = tok.parse::<f32>() {
            level = Some(v);
            break;
        }
    }
    Some(Volume {
        level: level.unwrap_or(0.0),
        muted: text.contains("MUTED"),
    })
}

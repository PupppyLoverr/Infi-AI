//! Live activities for the Dynamic Island: connected agents (from
//! cosmos-agentd's `agent-live` files), pending approval cards, and the
//! MPRIS now-playing player on the session bus.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::notify::Notification;

#[derive(Clone, Debug, PartialEq)]
pub struct AgentActivity {
    pub agent: String,
    pub started: u64,
    pub tool: String,
    pub busy: bool,
    pub calls: u64,
    pub paused: bool,
    /// Stopped from the island: agentd refuses its further tool calls.
    pub stopped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub bus: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

fn live_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
        .join(".local/state/cosmos/agent-live")
}

/// Same file-name mapping cosmos-agentd uses for `<agent>.json`.
fn safe_name(agent: &str) -> String {
    agent
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Connected agents. Records whose daemon pid is gone are stale leftovers
/// of a crash and are skipped.
pub fn scan_agents() -> Vec<AgentActivity> {
    let dir = live_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let pid = v["pid"].as_u64().unwrap_or(0);
        if pid == 0 || !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            continue;
        }
        let agent = v["agent"].as_str().unwrap_or("agent").to_string();
        out.push(AgentActivity {
            paused: dir.join(format!("{}.paused", safe_name(&agent))).exists(),
            stopped: dir.join(format!("{}.stopped", safe_name(&agent))).exists(),
            agent,
            started: v["started"].as_u64().unwrap_or(0),
            tool: v["tool"].as_str().unwrap_or("").to_string(),
            busy: v["busy"].as_bool().unwrap_or(false),
            calls: v["calls"].as_u64().unwrap_or(0),
        });
    }
    out.sort_by(|a, b| a.agent.cmp(&b.agent));
    out
}

/// Pause holds the agent's next tool call inside cosmos-agentd until resumed.
pub fn set_paused(agent: &str, paused: bool) {
    let dir = live_dir();
    let flag = dir.join(format!("{}.paused", safe_name(agent)));
    let r = if paused {
        std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&flag, b""))
    } else {
        std::fs::remove_file(&flag)
    };
    if let Err(e) = r {
        tracing::warn!("agent pause flag {}: {e}", flag.display());
    }
}

/// Stop refuses every further tool call of the agent inside cosmos-agentd
/// (and releases a held paused call, which is then refused too).
pub fn set_stopped(agent: &str) {
    let dir = live_dir();
    let r = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(dir.join(format!("{}.stopped", safe_name(agent))), b""));
    if let Err(e) = r {
        tracing::warn!("agent stop flag {agent}: {e}");
    }
    set_paused(agent, false);
}

/// "4:07" / "1:02:33" since `started`.
pub fn elapsed(started: u64) -> String {
    let s = now_secs().saturating_sub(started);
    if s < 3600 {
        format!("{}:{:02}", s / 60, s % 60)
    } else {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    }
}

/// Approval cards still waiting for a decision.
pub fn pending_approvals(notifs: &[Notification]) -> Vec<&Notification> {
    notifs
        .iter()
        .filter(|n| n.critical && !n.actions.is_empty())
        .collect()
}

/// Polls the session bus every 2s and reports the most relevant player
/// (Playing beats Paused; Stopped players are ignored) when it changes.
pub fn start_mpris(tx: calloop::channel::Sender<Option<Media>>) {
    std::thread::spawn(move || {
        let conn = match zbus::blocking::Connection::session() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("mpris: no session bus: {e}");
                return;
            }
        };
        let mut last: Option<Media> = None;
        loop {
            let now = poll_players(&conn).unwrap_or(None);
            if now != last {
                if tx.send(now.clone()).is_err() {
                    return;
                }
                last = now;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

fn player_proxy<'a>(
    conn: &zbus::blocking::Connection,
    bus: &'a str,
) -> zbus::Result<zbus::blocking::Proxy<'a>> {
    zbus::blocking::Proxy::new(
        conn,
        bus,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
}

fn poll_players(conn: &zbus::blocking::Connection) -> zbus::Result<Option<Media>> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(conn)?;
    let mut best: Option<Media> = None;
    for name in dbus.list_names()? {
        let bus = name.as_str();
        if !bus.starts_with("org.mpris.MediaPlayer2.") {
            continue;
        }
        let Ok(p) = player_proxy(conn, bus) else {
            continue;
        };
        let status: String = p.get_property("PlaybackStatus").unwrap_or_default();
        if status != "Playing" && status != "Paused" {
            continue;
        }
        let meta: HashMap<String, zbus::zvariant::OwnedValue> =
            p.get_property("Metadata").unwrap_or_default();
        let title = meta
            .get("xesam:title")
            .and_then(|v| <&str>::try_from(&**v).ok())
            .unwrap_or("")
            .to_string();
        let artist = meta
            .get("xesam:artist")
            .and_then(|v| <&zbus::zvariant::Array>::try_from(&**v).ok())
            .map(|a| {
                a.iter()
                    .filter_map(|x| <&str>::try_from(x).ok())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let m = Media {
            bus: bus.to_string(),
            title: if title.is_empty() {
                bus.trim_start_matches("org.mpris.MediaPlayer2.")
                    .to_string()
            } else {
                title
            },
            artist,
            playing: status == "Playing",
        };
        if best
            .as_ref()
            .map(|b| !b.playing && m.playing)
            .unwrap_or(true)
        {
            best = Some(m);
        }
    }
    Ok(best)
}

pub fn play_pause(bus: String) {
    std::thread::spawn(move || {
        let r = zbus::blocking::Connection::session().and_then(|c| {
            player_proxy(&c, &bus)?
                .call_method("PlayPause", &())
                .map(|_| ())
        });
        if let Err(e) = r {
            tracing::warn!("mpris PlayPause {bus}: {e}");
        }
    });
}

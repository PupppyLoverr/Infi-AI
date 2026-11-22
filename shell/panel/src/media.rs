//! Now playing, from any MPRIS player on the session bus (Firefox, Spotify,
//! mpv with its plugin, VLC, ...). Read in a background thread every couple of
//! seconds; the island shows it and its buttons call the player's own
//! PlayPause / Next / Previous.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use dbus::arg::{prop_cast, PropMap};
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::Connection;

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    /// The player's bus name, to send controls to.
    pub bus: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// MPRIS Position, microseconds.
    pub position_us: Option<i64>,
    /// `mpris:length` from Metadata, microseconds.
    pub length_us: Option<i64>,
}

impl Media {
    /// Identity for change detection (ignore ticking position).
    pub fn same_track(&self, other: &Self) -> bool {
        self.bus == other.bus
            && self.title == other.title
            && self.artist == other.artist
            && self.playing == other.playing
            && self.length_us == other.length_us
    }

    pub fn elapsed_label(&self) -> Option<(String, String, f32)> {
        let len = self.length_us.filter(|l| *l > 0)?;
        let pos = self.position_us.unwrap_or(0).clamp(0, len);
        let pct = (pos as f32 / len as f32).clamp(0.0, 1.0);
        Some((fmt_us(pos), fmt_us(len), pct))
    }
}

fn fmt_us(us: i64) -> String {
    let secs = (us / 1_000_000).max(0) as u64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Title and artist from an MPRIS `Metadata` map.
pub fn parse_metadata(meta: &PropMap) -> (String, String) {
    let title = prop_cast::<String>(meta, "xesam:title")
        .cloned()
        .unwrap_or_default();
    let artist = prop_cast::<Vec<String>>(meta, "xesam:artist")
        .map(|v| v.join(", "))
        .or_else(|| prop_cast::<String>(meta, "xesam:artist").cloned())
        .unwrap_or_default();
    (title, artist)
}

/// Choose what to show: a playing player first, else a paused one with a title.
pub fn pick(players: Vec<Media>) -> Option<Media> {
    let mut paused = None;
    for m in players {
        if m.playing {
            return Some(m);
        }
        if paused.is_none() && !m.title.is_empty() {
            paused = Some(m);
        }
    }
    paused
}

fn query(conn: &Connection) -> Option<Media> {
    let proxy = conn.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_millis(300),
    );
    let (names,): (Vec<String>,) = proxy
        .method_call("org.freedesktop.DBus", "ListNames", ())
        .ok()?;
    let mut players = Vec::new();
    for bus in names.into_iter().filter(|n| n.starts_with(PREFIX)) {
        let p = conn.with_proxy(bus.as_str(), PATH, Duration::from_millis(300));
        let status: String = p.get(PLAYER, "PlaybackStatus").unwrap_or_default();
        if status == "Stopped" || status.is_empty() {
            continue;
        }
        let meta: PropMap = p.get(PLAYER, "Metadata").unwrap_or_default();
        let (title, artist) = parse_metadata(&meta);
        let length_us = prop_cast::<i64>(&meta, "mpris:length")
            .copied()
            .or_else(|| prop_cast::<u64>(&meta, "mpris:length").map(|v| *v as i64));
        let position_us: Option<i64> = p.get(PLAYER, "Position").ok();
        players.push(Media {
            bus,
            title,
            artist,
            playing: status == "Playing",
            position_us,
            length_us,
        });
    }
    pick(players)
}

/// Poll in the background; `wake` is called when what is playing changes.
pub fn spawn_watcher(slot: Arc<Mutex<Option<Media>>>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let Ok(conn) = Connection::new_session() else {
            return;
        };
        loop {
            let now = query(&conn);
            let changed = slot.lock().map(|mut s| {
                let refresh = match (&*s, &now) {
                    (Some(a), Some(b)) => !a.same_track(b) || a.position_us != b.position_us,
                    (None, None) => false,
                    _ => true,
                };
                if refresh {
                    *s = now;
                }
                refresh
            });
            if changed.unwrap_or(false) {
                wake();
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    PlayPause,
    Next,
    Previous,
}

/// Send a control to the player, off the UI thread.
pub fn control(bus: String, what: Control) {
    std::thread::spawn(move || {
        let Ok(conn) = Connection::new_session() else {
            return;
        };
        let p = conn.with_proxy(bus.as_str(), PATH, Duration::from_millis(500));
        let method = match what {
            Control::PlayPause => "PlayPause",
            Control::Next => "Next",
            Control::Previous => "Previous",
        };
        if let Err(e) = p.method_call::<(), _, _, _>(PLAYER, method, ()) {
            eprintln!("solyra-panel: media {method}: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::arg::{RefArg, Variant};

    fn m(title: &str, playing: bool) -> Media {
        Media {
            bus: format!("{PREFIX}{title}"),
            title: title.into(),
            artist: String::new(),
            playing,
            position_us: None,
            length_us: None,
        }
    }

    #[test]
    fn elapsed_formats_and_pct() {
        let media = Media {
            bus: "x".into(),
            title: "FALLEN ANGEL".into(),
            artist: "JENNIE".into(),
            playing: true,
            position_us: Some(15_000_000),
            length_us: Some(203_000_000),
        };
        let (a, b, pct) = media.elapsed_label().unwrap();
        assert_eq!(a, "0:15");
        assert_eq!(b, "3:23");
        assert!((pct - 15.0 / 203.0).abs() < 0.01);
    }

    #[test]
    fn a_playing_player_wins_then_a_paused_one_with_a_title() {
        assert_eq!(pick(vec![m("a", false), m("b", true)]).unwrap().title, "b");
        assert_eq!(pick(vec![m("", false), m("c", false)]).unwrap().title, "c");
        assert!(pick(vec![m("", false)]).is_none());
        assert!(pick(vec![]).is_none());
    }

    #[test]
    fn metadata_parses_title_and_artists() {
        let mut meta = PropMap::new();
        meta.insert(
            "xesam:title".into(),
            Variant(Box::new("Song".to_string()) as Box<dyn RefArg>),
        );
        meta.insert(
            "xesam:artist".into(),
            Variant(Box::new(vec!["A".to_string(), "B".to_string()]) as Box<dyn RefArg>),
        );
        assert_eq!(parse_metadata(&meta), ("Song".into(), "A, B".into()));
        assert_eq!(
            parse_metadata(&PropMap::new()),
            (String::new(), String::new())
        );
    }
}

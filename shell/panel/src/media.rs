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
        players.push(Media {
            bus,
            title,
            artist,
            playing: status == "Playing",
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
                if *s != now {
                    *s = now;
                    true
                } else {
                    false
                }
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
        }
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

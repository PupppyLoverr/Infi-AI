//! Bundled app icons for the roles the shell itself knows (Files, Terminal,
//! Browser, Settings, ...).
//!
//! The installed icon theme on a Solyra image is Adwaita, whose app icons are
//! flat symbolic-ish glyphs with transparent margins. A dock needs icons with
//! real depth, so the panel carries a small curated set, baked to PNG from the
//! WhiteSur icon theme (GPL-3.0, see `assets/icons/README.md`). Anything not
//! in the set still falls back to the system theme on a neutral tile.

use std::sync::OnceLock;

use crate::canvas::{decode_png, Icon};

/// How much of the 192 px canvas the squircle body fills; the rest is the
/// icon's own soft shadow. The painter scales by `1 / BODY` so the *body*
/// lands exactly on the requested size.
pub const BODY: f32 = 168.0 / 192.0;

macro_rules! bundle {
    ($($name:literal),+ $(,)?) => {
        &[$(($name, include_bytes!(concat!("../assets/icons/", $name, ".png")) as &[u8])),+]
    };
}

const SET: &[(&str, &[u8])] = bundle![
    "files", "terminal", "browser", "firefox", "chrome", "settings", "overview", "editor",
    "trash", "wallpaper", "calculator", "music", "photos", "mail", "appstore", "monitor",
    "notes", "maps", "launchpad", "downloads", "viewer",
];

static DECODED: OnceLock<Vec<(&'static str, Icon)>> = OnceLock::new();

fn decoded() -> &'static [(&'static str, Icon)] {
    DECODED.get_or_init(|| {
        SET.iter()
            .filter_map(|(name, bytes)| decode_png(bytes).map(|ic| (*name, ic)))
            .collect()
    })
}

/// The bundled icon called `name`.
pub fn get(name: &str) -> Option<&'static Icon> {
    decoded().iter().find(|(n, _)| *n == name).map(|(_, ic)| ic)
}

/// Which bundled icon stands for a shell role or an application id, if any.
/// `key` is matched case-insensitively; first-party roles win, then well-known
/// application families by substring.
pub fn name_for(key: &str) -> Option<&'static str> {
    let k = key.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| k.contains(n));
    Some(match k.as_str() {
        "files" | "thunar" => "files",
        "terminal" | "foot" | "solyra-term" => "terminal",
        "browser" => "browser",
        "settings" => "settings",
        "overview" | "launcher" => "overview",
        "editor" | "gedit" => "editor",
        "trash" => "trash",
        "wallpaper" => "wallpaper",
        "start" | "launchpad" => "launchpad",
        "downloads" => "downloads",
        "viewer" | "ristretto" | "org.xfce.ristretto" => "viewer",
        _ if has(&["firefox"]) => "firefox",
        _ if has(&["chrome", "chromium"]) => "chrome",
        _ if has(&["thunar", "nautilus", "pcmanfm", "dolphin", "file-manager"]) => "files",
        _ if has(&["term", "konsole", "alacritty", "kitty", "wezterm"]) => "terminal",
        _ if has(&["solyra-browse", "epiphany", "web-browser", "falkon"]) => "browser",
        _ if has(&["settings", "control-center", "controlcenter"]) => "settings",
        _ if has(&["mousepad", "texteditor", "text-editor", "kate", "emacs", "gedit"]) => "editor",
        _ if has(&["calc"]) => "calculator",
        _ if has(&["music", "rhythmbox", "lollypop"]) => "music",
        _ if has(&["photo", "shotwell", "eog", "loupe", "gwenview"]) => "photos",
        _ if has(&["mail", "thunderbird", "evolution"]) => "mail",
        _ if has(&["software", "appstore", "flatpak"]) => "appstore",
        _ if has(&["monitor", "htop", "btop"]) => "monitor",
        _ if has(&["note", "remind", "todo"]) => "notes",
        _ if has(&["maps"]) => "maps",
        _ => return None,
    })
}

/// `(grow, lift)` for an icon: extra scale on top of `1 / BODY`, and how far
/// to raise it (as a fraction of its size). The trash can is a free-standing
/// object, not a squircle, so it is drawn taller and nudged up to look level
/// with the squircles beside it.
pub fn fit(name: &str) -> (f32, f32) {
    match name {
        "trash" => (1.10, 0.045),
        // A free-standing folder: the same visual weight as the squircles.
        "downloads" => (1.04, 0.0),
        _ => (1.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_icon_decodes_square_with_alpha() {
        assert_eq!(decoded().len(), SET.len(), "a bundled PNG failed to decode");
        for (name, ic) in decoded() {
            assert_eq!((ic.width, ic.height), (192, 192), "{name}");
            assert!(ic.pixels.chunks_exact(4).any(|p| p[3] == 255), "{name}");
            assert_eq!(ic.pixels[3], 0, "{name}: corners are transparent");
        }
    }

    #[test]
    fn roles_and_app_ids_resolve() {
        assert_eq!(name_for("files"), Some("files"));
        assert_eq!(name_for("foot"), Some("terminal"));
        assert_eq!(name_for("firefox"), Some("firefox"));
        assert_eq!(name_for("org.mozilla.firefox"), Some("firefox"));
        assert_eq!(name_for("org.xfce.mousepad"), Some("editor"));
        assert_eq!(name_for("com.google.Chrome"), Some("chrome"));
        assert_eq!(name_for("solyra-settings"), Some("settings"));
        assert_eq!(name_for("solyra-agents"), None);
        assert_eq!(name_for("start"), Some("launchpad"));
        assert_eq!(name_for("downloads"), Some("downloads"));
        assert!(get("files").is_some() && get("nope").is_none());
    }

    #[test]
    fn every_mapped_name_is_bundled() {
        for key in [
            "files", "terminal", "browser", "settings", "overview", "editor", "trash",
            "wallpaper", "firefox", "chrome", "calc", "rhythmbox", "shotwell", "thunderbird",
            "flatpak", "htop", "notes", "maps",
        ] {
            let n = name_for(key).unwrap_or_else(|| panic!("{key} unmapped"));
            assert!(get(n).is_some(), "{key} -> {n} not bundled");
        }
    }
}

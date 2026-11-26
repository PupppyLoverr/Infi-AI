//! Bundled app icons for the roles the shell itself knows (Files, Terminal,
//! Browser, Settings, ...).
//!
//! The installed icon theme on a Solyra image is Adwaita, whose app icons are
//! flat symbolic-ish glyphs with transparent margins. A dock needs icons with
//! real depth, so the panel carries Solyra's own set: macOS-style squircles
//! drawn by `assets/icons/make_icons.py` (first-party art, no icon library).
//! Anything not in the set falls back to the system theme on a neutral tile.
//!
//! **Your own icons.** PNGs in `~/.local/share/solyra/icons/` win over the
//! bundled set: name a file after a role (`files.png`, `browser.png`,
//! `terminal.png`, `trash.png`, `downloads.png`, `launchpad.png`, ...) or an
//! application id (`org.mozilla.firefox.png`). Any size works; macOS-style
//! icons with a transparent margin are measured and scaled so their body
//! matches the bundled squircles. These files stay on your machine; nothing
//! from a third-party icon site is shipped with Solyra.

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
    "files",
    "terminal",
    "browser",
    "settings",
    "overview",
    "editor",
    "trash",
    "wallpaper",
    "calculator",
    "music",
    "photos",
    "mail",
    "appstore",
    "monitor",
    "notes",
    "maps",
    "launchpad",
    "downloads",
    "viewer",
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

/// The folder for the person's own icons.
pub fn user_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })?;
    Some(base.join("solyra/icons"))
}

type UserCache = std::sync::Mutex<std::collections::HashMap<String, Option<(&'static Icon, f32)>>>;
static USER: OnceLock<UserCache> = OnceLock::new();

/// A user icon for `key` (a role or an application id), with the `grow`
/// that puts its body at the bundled squircles' size. Looked up once per key.
pub fn user(key: &str) -> Option<(&'static Icon, f32)> {
    let k = key.to_ascii_lowercase();
    let cache = USER.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok()?.get(&k) {
        return *hit;
    }
    let dir = user_dir();
    let mut names = vec![k.clone()];
    if let Some(n) = name_for(&k) {
        names.push(n.to_string());
    }
    let found = dir.and_then(|d| {
        names.iter().find_map(|n| {
            let bytes = std::fs::read(d.join(format!("{n}.png"))).ok()?;
            let ic = shrink(decode_png(&bytes)?, 256);
            let grow = BODY / body_fraction(&ic);
            Some((&*Box::leak(Box::new(ic)), grow.clamp(0.8, 1.4)))
        })
    });
    if let Ok(mut c) = cache.lock() {
        c.insert(k, found);
    }
    found
}

/// How much of the icon's width its opaque body takes (macOS icons leave a
/// transparent margin for the shadow).
pub fn body_fraction(ic: &Icon) -> f32 {
    let (w, h) = (ic.width as usize, ic.height as usize);
    let (mut x0, mut x1) = (w, 0);
    for y in 0..h {
        for x in 0..w {
            if ic.pixels[(y * w + x) * 4 + 3] > 200 {
                x0 = x0.min(x);
                x1 = x1.max(x);
            }
        }
    }
    if x1 <= x0 {
        return 1.0;
    }
    ((x1 - x0 + 1) as f32 / w as f32).clamp(0.3, 1.0)
}

/// Box-filter an icon down to at most `max` pixels square (alpha-weighted).
pub fn shrink(ic: Icon, max: u32) -> Icon {
    if ic.width <= max && ic.height <= max {
        return ic;
    }
    let k = ic.width.max(ic.height).div_ceil(max);
    let (nw, nh) = (ic.width / k, ic.height / k);
    let mut px = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0f32; 4];
            for dy in 0..k {
                for dx in 0..k {
                    let i = (((y * k + dy) * ic.width + x * k + dx) * 4) as usize;
                    let a = ic.pixels[i + 3] as f32;
                    for c in 0..3 {
                        acc[c] += ic.pixels[i + c] as f32 * a;
                    }
                    acc[3] += a;
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            let n = (k * k) as f32;
            if acc[3] > 0.0 {
                for c in 0..3 {
                    px[o + c] = (acc[c] / acc[3]).round() as u8;
                }
            }
            px[o + 3] = (acc[3] / n).round() as u8;
        }
    }
    Icon {
        width: nw,
        height: nh,
        pixels: px,
    }
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
        // Any web browser gets Solyra's browser tile.
        _ if has(&["firefox", "chrome", "chromium", "browser"]) => "browser",
        _ if has(&["thunar", "nautilus", "pcmanfm", "dolphin", "file-manager"]) => "files",
        _ if has(&["term", "konsole", "alacritty", "kitty", "wezterm"]) => "terminal",
        _ if has(&["solyra-browse", "epiphany", "web-browser", "falkon"]) => "browser",
        _ if has(&["settings", "control-center", "controlcenter"]) => "settings",
        _ if has(&[
            "mousepad",
            "texteditor",
            "text-editor",
            "kate",
            "emacs",
            "gedit",
        ]) =>
        {
            "editor"
        }
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
        "trash" => (1.04, 0.02),
        // A free-standing folder: the same visual weight as the squircles.
        "downloads" => (1.04, 0.0),
        _ => (1.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_icons_are_measured_and_shrunk() {
        // A 1024 canvas whose opaque body is the middle 824 px, like a
        // macOS app icon.
        let n = 1024u32;
        let mut px = vec![0u8; (n * n * 4) as usize];
        for y in 100..924u32 {
            for x in 100..924u32 {
                let i = ((y * n + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[200, 60, 60, 255]);
            }
        }
        let ic = shrink(
            Icon {
                width: n,
                height: n,
                pixels: px,
            },
            256,
        );
        assert_eq!((ic.width, ic.height), (256, 256));
        let f = body_fraction(&ic);
        assert!((f - 824.0 / 1024.0).abs() < 0.02, "{f}");
    }

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
        assert_eq!(name_for("firefox"), Some("browser"));
        assert_eq!(name_for("org.mozilla.firefox"), Some("browser"));
        assert_eq!(name_for("org.xfce.mousepad"), Some("editor"));
        assert_eq!(name_for("com.google.Chrome"), Some("browser"));
        assert_eq!(name_for("solyra-settings"), Some("settings"));
        assert_eq!(name_for("solyra-agents"), None);
        assert_eq!(name_for("start"), Some("launchpad"));
        assert_eq!(name_for("downloads"), Some("downloads"));
        assert!(get("files").is_some() && get("nope").is_none());
    }

    #[test]
    fn every_mapped_name_is_bundled() {
        for key in [
            "files",
            "terminal",
            "browser",
            "settings",
            "overview",
            "editor",
            "trash",
            "wallpaper",
            "firefox",
            "chrome",
            "calc",
            "rhythmbox",
            "shotwell",
            "thunderbird",
            "flatpak",
            "htop",
            "notes",
            "maps",
        ] {
            let n = name_for(key).unwrap_or_else(|| panic!("{key} unmapped"));
            assert!(get(n).is_some(), "{key} -> {n} not bundled");
        }
    }
}

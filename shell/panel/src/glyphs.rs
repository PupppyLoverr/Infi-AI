//! UI glyphs: Solar (Linear) by 480 Design, CC BY 4.0 (the set zeron uses),
//! baked to 48 px alpha masks by `assets/glyphs/bake_solar.py`.

use std::sync::OnceLock;

use crate::canvas::{decode_png, Icon};

const SET: &[(&str, &[u8])] = &[
    ("agents", include_bytes!("../assets/glyphs/agents.png")),
    ("bell", include_bytes!("../assets/glyphs/bell.png")),
    (
        "bluetooth",
        include_bytes!("../assets/glyphs/bluetooth.png"),
    ),
    ("clock", include_bytes!("../assets/glyphs/clock.png")),
    ("cloud", include_bytes!("../assets/glyphs/cloud.png")),
    ("control", include_bytes!("../assets/glyphs/control.png")),
    ("document", include_bytes!("../assets/glyphs/document.png")),
    ("download", include_bytes!("../assets/glyphs/download.png")),
    ("folder", include_bytes!("../assets/glyphs/folder.png")),
    ("gallery", include_bytes!("../assets/glyphs/gallery.png")),
    ("home", include_bytes!("../assets/glyphs/home.png")),
    ("laptop", include_bytes!("../assets/glyphs/laptop.png")),
    ("lock", include_bytes!("../assets/glyphs/lock.png")),
    ("monitor", include_bytes!("../assets/glyphs/monitor.png")),
    ("moon", include_bytes!("../assets/glyphs/moon.png")),
    ("note", include_bytes!("../assets/glyphs/note.png")),
    ("power", include_bytes!("../assets/glyphs/power.png")),
    ("search", include_bytes!("../assets/glyphs/search.png")),
    ("server", include_bytes!("../assets/glyphs/server.png")),
    ("settings", include_bytes!("../assets/glyphs/settings.png")),
    ("share", include_bytes!("../assets/glyphs/share.png")),
    ("star", include_bytes!("../assets/glyphs/star.png")),
    ("sun", include_bytes!("../assets/glyphs/sun.png")),
    ("tag", include_bytes!("../assets/glyphs/tag.png")),
    ("trash", include_bytes!("../assets/glyphs/trash.png")),
    ("volume", include_bytes!("../assets/glyphs/volume.png")),
    (
        "volume-low",
        include_bytes!("../assets/glyphs/volume-low.png"),
    ),
    (
        "volume-mute",
        include_bytes!("../assets/glyphs/volume-mute.png"),
    ),
    ("widget", include_bytes!("../assets/glyphs/widget.png")),
    ("wifi", include_bytes!("../assets/glyphs/wifi.png")),
    (
        "wifi-full",
        include_bytes!("../assets/glyphs/wifi-full.png"),
    ),
];

static DECODED: OnceLock<Vec<(&'static str, Icon)>> = OnceLock::new();

/// The mask called `name`.
pub fn get(name: &str) -> Option<&'static Icon> {
    DECODED
        .get_or_init(|| {
            SET.iter()
                .filter_map(|(n, b)| decode_png(b).map(|ic| (*n, ic)))
                .collect()
        })
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, ic)| ic)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_glyph_decodes() {
        for (n, _) in super::SET {
            let g = super::get(n).expect(n);
            assert_eq!((g.width, g.height), (48, 48), "{n}");
        }
    }
}

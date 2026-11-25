//! The shell's bundled app and place icons (WhiteSur, GPL-3.0; see
//! `shell/panel/assets/icons/README.md`), for Solyra apps, so an app and the
//! dock show the same folder, trash and app artwork.

use std::sync::{Arc, OnceLock};

use zgx::image::ImageData;

macro_rules! bundle {
    ($($name:literal),+ $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../../shell/panel/assets/icons/", $name, ".png")) as &[u8])),+]
    };
}

const SET: &[(&str, &[u8])] = bundle!["files", "downloads", "trash", "settings", "terminal", "editor", "viewer", "music"];

fn decode(bytes: &[u8]) -> Option<ImageData> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    ImageData::new(info.width, info.height, buf).ok()
}

static DECODED: OnceLock<Vec<(&'static str, Arc<ImageData>)>> = OnceLock::new();

/// The bundled icon `name` (`files` is the folder), decoded once.
pub fn get(name: &str) -> Option<Arc<ImageData>> {
    DECODED
        .get_or_init(|| {
            SET.iter()
                .filter_map(|(n, b)| decode(b).map(|i| (*n, Arc::new(i))))
                .collect()
        })
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, i)| i.clone())
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_icon_decodes() {
        for (name, _) in super::SET {
            assert!(super::get(name).is_some(), "{name}");
        }
        assert!(super::get("nope").is_none());
    }
}

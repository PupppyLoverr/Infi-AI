//! Shared app icons: Solar glyphs (CC BY 4.0, 480 Design), as bundled by
//! zeron (github.com/zeronsh/zeron, MIT). Embedded SVG, tinted per use.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use zgx::compose::prelude::*;
use zgx::scene::Color;
use zgx::svg::SvgData;

fn source(name: &str) -> &'static [u8] {
    match name {
        "alt-arrow-left" => include_bytes!("../assets/icons/alt-arrow-left.svg"),
        "alt-arrow-right" => include_bytes!("../assets/icons/alt-arrow-right.svg"),
        "clock-circle" => include_bytes!("../assets/icons/clock-circle.svg"),
        "document" => include_bytes!("../assets/icons/document.svg"),
        "download" => include_bytes!("../assets/icons/download.svg"),
        "eye" => include_bytes!("../assets/icons/eye.svg"),
        "eye-closed" => include_bytes!("../assets/icons/eye-closed.svg"),
        "folder" => include_bytes!("../assets/icons/folder.svg"),
        "hard-drive" => include_bytes!("../assets/icons/hard-drive.svg"),
        "home" => include_bytes!("../assets/icons/home.svg"),
        "info-circle" => include_bytes!("../assets/icons/info-circle.svg"),
        "laptop" => include_bytes!("../assets/icons/laptop.svg"),
        "list" => include_bytes!("../assets/icons/list.svg"),
        "magnifer" => include_bytes!("../assets/icons/magnifer.svg"),
        "monitor" => include_bytes!("../assets/icons/monitor.svg"),
        "refresh" => include_bytes!("../assets/icons/refresh.svg"),
        "sort" => include_bytes!("../assets/icons/sort.svg"),
        "split-columns" => include_bytes!("../assets/icons/split-columns.svg"),
        "star" => include_bytes!("../assets/icons/star.svg"),
        "tag" => include_bytes!("../assets/icons/tag.svg"),
        "trash-bin-minimalistic" => include_bytes!("../assets/icons/trash-bin-minimalistic.svg"),
        "widget" => include_bytes!("../assets/icons/widget.svg"),
        _ => include_bytes!("../assets/icons/info-circle.svg"),
    }
}

fn data(name: &str) -> Arc<SvgData> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<SvgData>>>> = OnceLock::new();
    let mut map = CACHE.get_or_init(Default::default).lock().unwrap();
    map.entry(name.to_string())
        .or_insert_with(|| Arc::new(SvgData::new(source(name)).expect("bundled icon")))
        .clone()
}

/// Icon `name`, `size` px square, tinted `color`.
pub fn icon(name: &str, size: f32, color: Color) -> View {
    svg(name.to_string(), data(name)).w(size).h(size).svg_tint(color)
}

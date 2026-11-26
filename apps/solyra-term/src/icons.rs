//! The terminal's icon set: zeron's (github.com/zeronsh/zeron, MIT), which is
//! Solar (CC BY 4.0, 480 Design) plus the harness marks. Embedded, drawn
//! as SVG and tinted.

use std::sync::{Arc, OnceLock};
use std::collections::HashMap;
use std::sync::Mutex;

use zgx::compose::prelude::*;
use zgx::scene::Color;
use zgx::svg::SvgData;

fn source(name: &str) -> &'static [u8] {
    match name {
        "sidebar" => include_bytes!("../assets/icons/sidebar-minimalistic-left.svg"),
        "plus" => include_bytes!("../assets/icons/plus.svg"),
        "split-columns" => include_bytes!("../assets/icons/split-columns.svg"),
        "split-rows" => include_bytes!("../assets/icons/split-rows.svg"),
        "terminal" => include_bytes!("../assets/icons/terminal.svg"),
        "claude" => include_bytes!("../assets/icons/claude-mark.svg"),
        "openai" => include_bytes!("../assets/icons/openai-mark.svg"),
        "opencode" => include_bytes!("../assets/icons/opencode-mark.svg"),
        "arrow-up" => include_bytes!("../assets/icons/arrow-up.svg"),
        _ => include_bytes!("../assets/icons/stop.svg"),
    }
}

fn data(name: &str) -> Arc<SvgData> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<SvgData>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut map = cache.lock().unwrap();
    map.entry(name.to_string())
        .or_insert_with(|| Arc::new(SvgData::new(source(name)).expect("bundled icon")))
        .clone()
}

/// An icon `size` px square, tinted `color`.
pub fn icon(name: &str, size: f32, color: Color) -> View {
    svg(name.to_string(), data(name)).w(size).h(size).svg_tint(color)
}

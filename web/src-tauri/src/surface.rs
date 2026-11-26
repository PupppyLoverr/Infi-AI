//! Shell surfaces: Tauri webviews shown as wlr-layer-shell surfaces.
//!
//! gtk-layer-shell must configure a window before it is realized, and
//! Tauri's window already is by the time we see it. So each webview's
//! container moves into a fresh GTK window that is made a layer surface
//! first (and sized explicitly: a WebKit view requests about 0x0).

use std::cell::RefCell;
use std::collections::HashMap;

use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

/// How one shell surface sits on screen.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub label: &'static str,
    pub namespace: &'static str,
    pub layer: Layer,
    pub top: Option<i32>,
    pub bottom: Option<i32>,
    pub left: Option<i32>,
    pub right: Option<i32>,
    pub width: i32,
    pub height: i32,
    pub keyboard: KeyboardMode,
    /// Shown at start (dock, island) or on demand (spotlight, control).
    pub visible: bool,
}

pub const SPECS: &[Spec] = &[
    Spec {
        label: "dock",
        namespace: "solyra-web-dock",
        layer: Layer::Top,
        top: None,
        bottom: Some(8),
        left: None,
        right: None,
        width: 900,
        height: 150,
        keyboard: KeyboardMode::None,
        visible: true,
    },
    Spec {
        label: "island",
        namespace: "solyra-web-island",
        layer: Layer::Overlay,
        top: Some(0),
        bottom: None,
        left: None,
        right: None,
        width: 720,
        height: 440,
        keyboard: KeyboardMode::OnDemand,
        visible: true,
    },
    Spec {
        label: "spotlight",
        namespace: "solyra-web-spotlight",
        layer: Layer::Overlay,
        top: Some(0),
        bottom: None,
        left: None,
        right: None,
        width: 760,
        height: 560,
        keyboard: KeyboardMode::Exclusive,
        visible: false,
    },
    Spec {
        label: "control",
        namespace: "solyra-web-control",
        layer: Layer::Overlay,
        top: Some(34),
        bottom: None,
        left: None,
        right: Some(10),
        width: 380,
        height: 620,
        keyboard: KeyboardMode::OnDemand,
        visible: false,
    },
];

thread_local! {
    static WINDOWS: RefCell<HashMap<&'static str, gtk::Window>> = RefCell::new(HashMap::new());
}

/// Move `webview_window`'s webview into a layer-shell window per `spec`.
pub fn adopt(webview_window: &tauri::WebviewWindow, spec: Spec) -> tauri::Result<()> {
    let gw = webview_window.gtk_window()?;
    let child = gw.child().expect("tauri window has a webview container");
    gw.remove(&child);
    let lw = gtk::Window::new(gtk::WindowType::Toplevel);
    lw.set_app_paintable(true);
    if let Some(visual) = GtkWindowExt::screen(&lw).and_then(|s| s.rgba_visual()) {
        lw.set_visual(Some(&visual));
    }
    lw.init_layer_shell();
    lw.set_namespace(spec.namespace);
    lw.set_layer(spec.layer);
    lw.set_exclusive_zone(-1);
    for (edge, m) in [
        (Edge::Top, spec.top),
        (Edge::Bottom, spec.bottom),
        (Edge::Left, spec.left),
        (Edge::Right, spec.right),
    ] {
        if let Some(m) = m {
            lw.set_anchor(edge, true);
            lw.set_layer_shell_margin(edge, m);
        }
    }
    lw.set_keyboard_mode(spec.keyboard);
    lw.set_default_size(spec.width, spec.height);
    lw.set_size_request(spec.width, spec.height);
    lw.add(&child);
    if spec.visible {
        lw.show_all();
    } else {
        child.show_all();
    }
    WINDOWS.with(|w| w.borrow_mut().insert(spec.label, lw));
    Ok(())
}

fn with_window(label: &str, f: impl FnOnce(&gtk::Window)) {
    WINDOWS.with(|w| {
        if let Some(win) = w.borrow().get(label) {
            f(win);
        }
    });
}

pub fn show(label: &str, on: bool) {
    with_window(label, |w| if on { w.show_all() } else { w.hide() });
}

pub fn toggle(label: &str) -> bool {
    let mut now = false;
    with_window(label, |w| {
        now = !w.is_visible();
        if now {
            w.show_all();
        } else {
            w.hide();
        }
    });
    now
}

pub fn resize(label: &str, width: i32, height: i32) {
    with_window(label, |w| {
        w.set_size_request(width.max(1), height.max(1));
        w.resize(width.max(1), height.max(1));
    });
}

/// Only these rectangles take the pointer; elsewhere clicks pass through
/// to whatever is below (the transparent parts of the island, the dock's
/// magnification room).
pub fn input_rects(label: &str, rects: &[[i32; 4]]) {
    with_window(label, |w| {
        let region = gtk::cairo::Region::create();
        for r in rects {
            let _ = region.union_rectangle(&gtk::cairo::RectangleInt::new(r[0], r[1], r[2], r[3]));
        }
        if let Some(gdk) = w.window() {
            gdk.input_shape_combine_region(&region, 0, 0);
        }
    });
}

/// Glass behind exactly these rounded boxes (x, y, w, h, radius).
pub fn glass(label: &str, boxes: &[[i32; 5]], preset: &str) {
    let p = crate::glass::preset(preset);
    let probe = [[0, 0, 100, 50, 0], [800, 100, 100, 50, 0]];
    let boxes: &[[i32; 5]] = if label == "dock" && std::env::var_os("SOLYRA_WEB_PROBE").is_some() { &probe } else { boxes };
    let shapes: Vec<crate::glass_proto::Shape> = boxes
        .iter()
        .map(|b| crate::glass_proto::Shape { x: b[0], y: b[1], w: b[2], h: b[3], r: b[4] as f32, preset: p })
        .collect();
    if std::env::var_os("SOLYRA_WEB_DEBUG").is_some() {
        eprintln!("glass {label}: {boxes:?} {preset}");
    }
    with_window(label, |w| crate::glass::set(label, w, &shapes));
}

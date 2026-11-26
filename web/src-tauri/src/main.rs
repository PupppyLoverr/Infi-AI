//! solyra-web: Solyra's web-rendered shell surfaces.
use gtk::prelude::*;
use gtk_layer_shell::{Edge, Layer, LayerShell};
use tauri::{WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let win = WebviewWindowBuilder::new(app, "dock", WebviewUrl::App("index.html#dock".into()))
                .transparent(true)
                .decorations(false)
                .visible(false)
                .inner_size(480.0, 80.0)
                .build()?;
            if std::env::var("SOLYRA_WEB_NO_LAYER").is_ok() {
                win.show()?;
                return Ok(());
            }
            // gtk-layer-shell must configure a window before it is realized,
            // and Tauri's window already is. So the webview moves into a
            // fresh GTK window that is made a layer surface first.
            let gw = win.gtk_window()?;
            let child = gw.child().expect("tauri window has a webview container");
            gw.remove(&child);
            let lw = gtk::Window::new(gtk::WindowType::Toplevel);
            lw.set_app_paintable(true);
            if let Some(visual) = gtk::prelude::GtkWindowExt::screen(&lw).and_then(|s| s.rgba_visual()) {
                lw.set_visual(Some(&visual));
            }
            lw.init_layer_shell();
            lw.set_namespace("solyra-web-dock");
            lw.set_layer(Layer::Overlay);
            lw.set_anchor(Edge::Top, true);
            lw.set_layer_shell_margin(Edge::Top, 60);
            lw.set_default_size(480, 80);
            lw.set_size_request(480, 80);
            lw.add(&child);
            lw.show_all();
            std::mem::forget(lw);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("solyra-web failed");
}

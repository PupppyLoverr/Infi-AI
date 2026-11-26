//! Shape-accurate glass for the web surfaces.
//!
//! A layer surface is a rectangle, but the dock, Spotlight and Control
//! Centre are rounded cards on a transparent surface. The compositor's
//! `solyra_glass_v1` takes up to four rounded boxes and blurs (and lenses)
//! only inside them, so the React side measures its cards and Rust sends the
//! boxes on GTK's own `wl_surface`, through a second event queue on GTK's
//! Wayland connection.

use std::cell::RefCell;
use std::collections::HashMap;

use glib::translate::ToGlibPtr;
use gtk::prelude::*;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use crate::glass_proto::{sanitize, Preset, Shape, SolyraGlassManagerV1, SolyraGlassSurfaceV1};

struct St;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for St {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
wayland_client::delegate_noop!(St: ignore SolyraGlassManagerV1);
wayland_client::delegate_noop!(St: ignore SolyraGlassSurfaceV1);
wayland_client::delegate_noop!(St: ignore WlSurface);

struct Glass {
    conn: Connection,
    queue: EventQueue<St>,
    manager: SolyraGlassManagerV1,
    /// Per surface label: the wl_surface pointer it was made for, and the object.
    surfaces: HashMap<String, (usize, SolyraGlassSurfaceV1)>,
}

thread_local! {
    static GLASS: RefCell<Option<Result<Glass, String>>> = const { RefCell::new(None) };
}

fn connect(win: &gtk::Window) -> Result<Glass, String> {
    let display = win.display();
    let raw = unsafe { { let d: *mut gtk::gdk::ffi::GdkDisplay = display.to_glib_none().0; gdk_wayland_sys::gdk_wayland_display_get_wl_display(d as *mut _) } };
    if raw.is_null() {
        return Err("not a Wayland display".into());
    }
    let backend = unsafe { wayland_backend::client::Backend::from_foreign_display(raw as *mut wayland_sys::client::wl_display) };
    let conn = Connection::from_backend(backend);
    let (globals, queue) = registry_queue_init::<St>(&conn).map_err(|e| e.to_string())?;
    let manager: SolyraGlassManagerV1 = globals
        .bind(&queue.handle(), 1..=2, ())
        .map_err(|_| "compositor has no solyra_glass_manager_v1".to_string())?;
    Ok(Glass { conn, queue, manager, surfaces: HashMap::new() })
}

/// Make the glass on surface `label` exactly `shapes` (empty = none).
pub fn set(label: &str, win: &gtk::Window, shapes: &[Shape]) {
    let Some(gdk_win) = win.window() else { return };
    let ptr = unsafe { { let w: *mut gtk::gdk::ffi::GdkWindow = gdk_win.to_glib_none().0; gdk_wayland_sys::gdk_wayland_window_get_wl_surface(w as *mut _) } };
    if ptr.is_null() {
        return; // not mapped yet
    }
    GLASS.with(|g| {
        let mut g = g.borrow_mut();
        if g.is_none() {
            *g = Some(connect(win));
            if let Some(Err(e)) = g.as_ref() {
                eprintln!("solyra-web: glass unavailable ({e}); surfaces keep their CSS tint");
            }
        }
        let Some(Ok(glass)) = g.as_mut() else { return };
        let qh = glass.queue.handle();
        let stale = glass.surfaces.get(label).map_or(true, |(p, _)| *p != ptr as usize);
        if stale {
            let id = unsafe { wayland_backend::client::ObjectId::from_ptr(WlSurface::interface(), ptr as *mut _) };
            let Ok(id) = id else { return };
            let Ok(surface) = WlSurface::from_id(&glass.conn, id) else { return };
            let gs = glass.manager.get_glass_surface(&surface, &qh, ());
            if let Some((_, old)) = glass.surfaces.insert(label.to_string(), (ptr as usize, gs)) {
                old.destroy();
            }
        }
        let gs = &glass.surfaces[label].1;
        gs.reset();
        for s in sanitize(shapes) {
            gs.add_shape(s.x, s.y, s.w, s.h, s.r.round() as u32, s.preset);
        }
        let _ = glass.conn.flush();
        let _ = glass.queue.dispatch_pending(&mut St);
    });
    // The state is double-buffered: GTK's next frame commits it.
    win.queue_draw();
}

/// Preset ids from the protocol, by name (for the frontend).
pub fn preset(name: &str) -> Preset {
    match name {
        "clear" => Preset::Clear,
        "solid" => Preset::Solid,
        "dark" => Preset::Dark,
        "dock" => Preset::Dock,
        "island" => Preset::Island,
        "popover" => Preset::Popover,
        "window" => Preset::Window,
        _ => Preset::Regular,
    }
}

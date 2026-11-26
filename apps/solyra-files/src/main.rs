//! Solyra Files: a Finder-inspired file manager in zgx. Thunar stays the
//! image default and the fallback; this app is the native Solyra alternative.
//!
//! The layout follows pearOS/macOS Finder: traffic-light window controls,
//! a translucent sidebar, icon grid or list view, breadcrumbs, search in the
//! current folder, stars, recents, trash and restore. `--self-test` opens the
//! window for two seconds and exits 0 (CI).

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use zgx::{
    compose::prelude::*,
    input::{EventPhase, InputEvent, PointerButton},
    reactive::Signal,
    scene::Color,
};
use zgx_components::segmented;
use zgx_desktop::{Application, WindowOptions};

mod model;

use model::Entry;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Location {
    Recents,
    Home,
    Starred,
    Trash,
    Tag(String),
    Folder(PathBuf),
}

impl Location {
    fn title(&self) -> String {
        match self {
            Self::Recents => "Recents".into(),
            Self::Home => "Home".into(),
            Self::Starred => "Starred".into(),
            Self::Trash => "Trash".into(),
            Self::Tag(color) => {
                let mut chars = color.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_else(|| "Tagged".into())
            }
            Self::Folder(path) if path == Path::new("/") => "Computer".into(),
            Self::Folder(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
        }
    }
}

#[derive(Clone)]
struct FilesApp {
    location: Signal<Location>,
    revision: Signal<u32>,
    selected: Signal<Option<PathBuf>>,
    query: Signal<String>,
    mode: Signal<usize>,
    show_hidden: Signal<bool>,
    show_inspector: Signal<bool>,
    confirm_trash: Signal<Option<PathBuf>>,
    renaming: Signal<Option<PathBuf>>,
    rename_text: Signal<String>,
    status: Signal<String>,
    back: Rc<RefCell<Vec<Location>>>,
    forward: Rc<RefCell<Vec<Location>>>,
    last_click: Rc<RefCell<Option<(PathBuf, Instant)>>>,
    clipboard: Rc<RefCell<Option<(PathBuf, bool)>>>,
}

impl FilesApp {
    fn bump(&self) {
        self.revision.update(|n| *n = n.wrapping_add(1));
    }

    fn settle(&self) {
        self.selected.set(None);
        self.confirm_trash.set(None);
        self.renaming.set(None);
        self.bump();
    }

    fn navigate(&self, next: Location) {
        let current = self.location.get();
        if current == next {
            return;
        }
        self.back.borrow_mut().push(current);
        self.forward.borrow_mut().clear();
        self.location.set(next);
        self.status.set(String::new());
        self.settle();
    }

    fn go_back(&self) {
        let Some(previous) = self.back.borrow_mut().pop() else {
            return;
        };
        let current = self.location.get();
        self.forward.borrow_mut().push(current);
        self.location.set(previous);
        self.settle();
    }

    fn go_forward(&self) {
        let Some(next) = self.forward.borrow_mut().pop() else {
            return;
        };
        let current = self.location.get();
        self.back.borrow_mut().push(current);
        self.location.set(next);
        self.settle();
    }

    fn open_entry(&self, entry: &Entry) {
        if entry.is_dir {
            self.navigate(Location::Folder(entry.path.clone()));
            return;
        }
        if let Err(e) = model::open_with_system(&entry.path) {
            self.status
                .set(format!("Could not open {}: {e}", entry.name));
        }
    }

    fn tap_entry(&self, entry: &Entry) {
        let now = Instant::now();
        let mut last = self.last_click.borrow_mut();
        let double = last.as_ref().is_some_and(|(path, at)| {
            *path == entry.path && now.duration_since(*at) < Duration::from_millis(500)
        });
        if double {
            *last = None;
            drop(last);
            self.open_entry(entry);
        } else {
            *last = Some((entry.path.clone(), now));
            drop(last);
            self.selected.set(Some(entry.path.clone()));
        }
    }

    fn refresh(&self) {
        self.bump();
    }
}

fn rgb(hex: u32) -> Color {
    Color(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
        255,
    )
}

fn entry_for_path(path: &Path) -> Option<Entry> {
    let meta = fs::metadata(path).ok()?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Some(Entry {
        name,
        path: path.to_path_buf(),
        is_dir: meta.is_dir(),
        is_symlink: meta.file_type().is_symlink(),
        hidden: false,
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

fn load_entries(location: &Location, show_hidden: bool) -> Result<Vec<Entry>, String> {
    match location {
        Location::Home => model::read_entries(&model::home_dir(), show_hidden)
            .map_err(|e| format!("Could not read your home folder: {e}")),
        Location::Folder(path) => model::read_entries(path, show_hidden)
            .map_err(|e| format!("Could not read {}: {e}", path.display())),
        Location::Trash => {
            let Some(root) = model::trash_dir() else {
                return Err("Cannot find the trash folder because HOME is not set.".into());
            };
            model::read_entries(&root.join("files"), true)
                .map_err(|e| format!("Could not read the trash: {e}"))
        }
        Location::Recents => Ok(model::recent()
            .iter()
            .filter_map(|p| entry_for_path(p))
            .collect()),
        Location::Starred => Ok(model::starred()
            .iter()
            .filter_map(|p| entry_for_path(p))
            .collect()),
        Location::Tag(color) => Ok(model::paths_tagged(color)
            .iter()
            .filter_map(|p| entry_for_path(p))
            .collect()),
    }
}

fn location_dir(location: &Location) -> Option<PathBuf> {
    match location {
        Location::Home => Some(model::home_dir()),
        Location::Folder(path) => Some(path.clone()),
        _ => None,
    }
}

fn small_button(label: &str, on_click: impl FnMut() + 'static) -> View {
    button()
        .h(28.)
        .px(10.)
        .rounded(7.)
        .items_center()
        .justify_center()
        .border(0.)
        .child(text(label.to_string()).text_size(12.))
        .on_click(on_click)
}

fn icon_button(label: &str, on_click: impl FnMut() + 'static) -> View {
    button()
        .w(30.)
        .h(28.)
        .rounded(7.)
        .items_center()
        .justify_center()
        .border(0.)
        .child(text(label.to_string()).text_size(15.))
        .on_click(on_click)
}

/// Icons / List / Columns as one glass segmented capsule.
fn view_modes(app: &FilesApp) -> View {
    let mut r = row().gap(2.);
    for (i, name) in ["widget", "list", "split-columns"].into_iter().enumerate() {
        let (m, set) = (app.mode.clone(), app.mode.clone());
        let ink = solyra_ui::color(solyra_ui::current().theme.text);
        r = r.child(
            button()
                .w(34.)
                .h(26.)
                .rounded(13.)
                .items_center()
                .justify_center()
                .border(0.)
                .reactive_style(move || Styles::new().bg(if m.get() == i { Color(255, 255, 255, 46) } else { Color(0, 0, 0, 0) }))
                .child(solyra_ui::glyphs::icon(name, 16., ink))
                .on_click(move || { set.set(i); }),
        );
    }
    r
}

/// A toolbar button drawing a Solar glyph.
fn glyph_button(name: &str, on_click: impl FnMut() + 'static) -> View {
    let ink = solyra_ui::color(solyra_ui::current().theme.text);
    button()
        .w(32.)
        .h(28.)
        .rounded(14.)
        .items_center()
        .justify_center()
        .border(0.)
        .bg(Color(0, 0, 0, 0))
        .child(solyra_ui::glyphs::icon(name, 17., ink))
        .on_click(on_click)
}

fn traffic_dot(color: Color, label: &str, on_click: impl FnMut() + 'static) -> View {
    // pearOS window spec: 14 px lights with a 0.5 px dark rim.
    let d = solyra_ui::chrome::LIGHT;
    button()
        .w(d)
        .h(d)
        .rounded(d / 2.)
        .border(0.5)
        .border_color(Color(0, 0, 0, 115))
        .bg(color)
        .accessible_label(label.to_string())
        .on_click(on_click)
}

fn drag_area(view: View, window: zgx_desktop::WindowHandle) -> View {
    view.on_event(move |cx| {
        if cx.phase == EventPhase::Capture {
            return;
        }
        if matches!(
            cx.event,
            InputEvent::PointerDown {
                button: PointerButton::Primary,
                ..
            }
        ) {
            window.drag_window();
            cx.prevent_default();
        }
    })
}

/// The traffic lights, drawn at the top of the inset sidebar (macOS 27).
fn window_lights(window: zgx_desktop::WindowHandle) -> View {
    let close = {
        let window = window.clone();
        traffic_dot(solyra_ui::chrome::RED, "Close window", move || window.close())
    };
    let compact = {
        let window = window.clone();
        traffic_dot(solyra_ui::chrome::YELLOW, "Minimize window", move || {
            window.set_minimized(true);
            solyra_ui::chrome::niri_action(solyra_ui::chrome::MINIMIZE);
        })
    };
    let fullscreen = {
        let full = std::rc::Rc::new(std::cell::Cell::new(false));
        traffic_dot(solyra_ui::chrome::GREEN, "Toggle fullscreen", move || {
            let next = !full.get();
            full.set(next);
            window.set_fullscreen(next);
        })
    };
    row()
        .gap(solyra_ui::chrome::LIGHT_GAP)
        .items_center()
        .child(close)
        .child(compact)
        .child(fullscreen)
}

fn toolbar(app: &FilesApp, style: &solyra_ui::Style, window: zgx_desktop::WindowHandle) -> View {
    let close = {
        let window = window.clone();
        traffic_dot(solyra_ui::chrome::RED, "Close window", move || window.close())
    };
    let compact = {
        let window = window.clone();
        traffic_dot(solyra_ui::chrome::YELLOW, "Minimize window", move || {
            window.set_minimized(true);
            solyra_ui::chrome::niri_action(solyra_ui::chrome::MINIMIZE);
        })
    };
    let fullscreen = {
        let window = window.clone();
        let full = std::rc::Rc::new(std::cell::Cell::new(false));
        traffic_dot(solyra_ui::chrome::GREEN, "Toggle fullscreen", move || {
            let next = !full.get();
            full.set(next);
            window.set_fullscreen(next);
        })
    };
    let _traffic = row()
        .gap(solyra_ui::chrome::LIGHT_GAP)
        .items_center()
        .child(close)
        .child(compact)
        .child(fullscreen);

    let back = {
        let app = app.clone();
        let enabled = {
            let app = app.clone();
            move || {
                let _ = app.revision.get();
                !app.back.borrow().is_empty()
            }
        };
        glyph_button("alt-arrow-left", move || app.go_back()).disabled_when(move || !enabled())
    };
    let forward = {
        let app = app.clone();
        let enabled = {
            let app = app.clone();
            move || {
                let _ = app.revision.get();
                !app.forward.borrow().is_empty()
            }
        };
        glyph_button("alt-arrow-right", move || app.go_forward()).disabled_when(move || !enabled())
    };
    // macOS 26: toolbar controls sit in glass capsules on a clear toolbar.
    let capsule = |v: View| {
        v.h(32.)
            .px(4.)
            .rounded(16.)
            .items_center()
            .bg(style.card_bg())
            .border(1.)
            .border_color(style.hairline())
    };
    let nav = capsule(row().gap(2.).child(back).child(forward));
    let title = text_signal({
        let location = app.location.clone();
        move || location.get().title()
    })
    .font_bold()
    .text_size(13.)
    .truncate();

    let hidden = {
        let app = app.clone();
        let shown = app.show_hidden.clone();
        let ink = solyra_ui::color(solyra_ui::current().theme.text);
        button()
            .w(32.)
            .h(28.)
            .rounded(14.)
            .items_center()
            .justify_center()
            .border(0.)
            .bg(Color(0, 0, 0, 0))
            .child(switch(move || shown.get(), move |on, _cx| solyra_ui::glyphs::icon(if on { "eye" } else { "eye-closed" }, 17., ink)))
            .on_click(move || {
                let next = !app.show_hidden.get();
                app.show_hidden.set(next);
            })
    };
    let refresh = {
        let app = app.clone();
        glyph_button("refresh", move || app.refresh())
    };
    let inspector_btn = {
        let app = app.clone();
        glyph_button("info-circle", move || {
            let next = !app.show_inspector.get();
            app.show_inspector.set(next);
        })
    };
    let search = row()
        .w(200.)
        .h(32.)
        .px(12.)
        .gap(6.)
        .items_center()
        .rounded(16.)
        .bg(style.card_bg())
        .border(1.)
        .border_color(style.hairline())
        .child(solyra_ui::glyphs::icon("magnifer", 15., solyra_ui::color(style.theme.text_dim)))
        .child(text_input("Search", app.query.clone()).grow().bg(Color(0, 0, 0, 0)).border(0.));

    row()
        .w_full()
        .h(52.)
        .px(14.)
        .gap(10.)
        .items_center()
        .child(nav)
        .child(drag_area(title.w(150.), window.clone()))
        .child(drag_area(row().grow(), window.clone()))
        .child(capsule(row().gap(2.).child(hidden).child(refresh).child(inspector_btn)))
        .child(capsule(view_modes(app)))
        .child(search)
}

fn sidebar_button(
    app: &FilesApp,
    style: &solyra_ui::Style,
    icon: &str,
    label: &str,
    location: Location,
) -> View {
    let selected = {
        let (current, wanted) = (app.location.clone(), location.clone());
        move || current.get() == wanted
    };
    let theme = style.theme;
    let click = {
        let app = app.clone();
        move || app.navigate(location.clone())
    };
    button()
        .h(32.)
        .w_full()
        .px(10.)
        .rounded(8.)
        .border(0.)
        .items_center()
        .reactive_style(move || {
            if selected() {
                Styles::new()
                    .bg(solyra_ui::color(theme.selected_fill))
                    .text_color(solyra_ui::color(theme.text))
            } else {
                Styles::new()
                    .bg(Color(0, 0, 0, 0))
                    .text_color(solyra_ui::color(theme.text_med))
            }
        })
        .on_click(click)
        .child(
            row()
                .gap(8.)
                .items_center()
                .child(solyra_ui::glyphs::icon(icon, 16., solyra_ui::color(theme.accent)))
                .child(text(label.to_string()).text_size(13.).truncate()),
        )
}

fn tag_color(name: &str) -> Color {
    match name {
        "red" => rgb(0xff5f57),
        "orange" => rgb(0xff9f0a),
        "yellow" => rgb(0xffd60a),
        "green" => rgb(0x32d74b),
        "blue" => rgb(0x0a84ff),
        "purple" => rgb(0xbf5af2),
        _ => rgb(0x8e8e93),
    }
}

fn tag_sidebar_button(app: &FilesApp, style: &solyra_ui::Style, color_name: &str) -> View {
    let location = Location::Tag(color_name.to_string());
    let selected = {
        let (current, wanted) = (app.location.clone(), location.clone());
        move || current.get() == wanted
    };
    let theme = style.theme;
    let click = {
        let app = app.clone();
        move || app.navigate(location.clone())
    };
    let mut label = color_name.chars();
    let label = label
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + label.as_str())
        .unwrap_or_else(|| color_name.to_string());
    button()
        .h(28.)
        .w_full()
        .px(10.)
        .rounded(8.)
        .border(0.)
        .items_center()
        .reactive_style(move || {
            if selected() {
                Styles::new()
                    .bg(solyra_ui::color(theme.selected_fill))
                    .text_color(solyra_ui::color(theme.text))
            } else {
                Styles::new()
                    .bg(Color(0, 0, 0, 0))
                    .text_color(solyra_ui::color(theme.text_med))
            }
        })
        .on_click(click)
        .child(
            row()
                .gap(9.)
                .items_center()
                .child(div().w(9.).h(9.).rounded(5.).bg(tag_color(color_name)))
                .child(text(label).text_size(12.5).truncate()),
        )
}

fn sidebar_section(title: &str, style: &solyra_ui::Style, body: View) -> View {
    column()
        .gap(3.)
        .child(
            text(title.to_uppercase())
                .text_size(10.5)
                .font_bold()
                .text_color(solyra_ui::color(style.theme.text_dim))
                .px(10.)
                .pt(8.)
                .pb(2.),
        )
        .child(body)
}

fn mount_locations() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/")];
    let mut candidates = vec![PathBuf::from("/mnt"), PathBuf::from("/media")];
    if let Some(user) = std::env::var_os("USER") {
        candidates.push(PathBuf::from("/run/media").join(user));
    }
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/Volumes"));
    }
    for root in candidates {
        let Ok(items) = fs::read_dir(root) else {
            continue;
        };
        for item in items.flatten() {
            let path = item.path();
            if path.is_dir() && !roots.contains(&path) {
                roots.push(path);
            }
        }
    }
    roots
}

fn sidebar(app: &FilesApp, style: &solyra_ui::Style, lights: View) -> View {
    let home = model::home_dir();
    let mut favourites = column()
        .gap(2.)
        .child(sidebar_button(app, style, "clock-circle", "Recents", Location::Recents))
        .child(sidebar_button(app, style, "home", "Home", Location::Home));
    for (icon, name) in [("monitor", "Desktop"), ("document", "Documents"), ("download", "Downloads")] {
        let dir = home.join(name);
        if dir.is_dir() {
            favourites = favourites.child(sidebar_button(
                app,
                style,
                icon,
                name,
                Location::Folder(dir),
            ));
        }
    }
    favourites = favourites
        .child(sidebar_button(app, style, "star", "Starred", Location::Starred))
        .child(sidebar_button(app, style, "trash-bin-minimalistic", "Trash", Location::Trash));
    let locations = mount_locations()
        .into_iter()
        .take(8)
        .fold(column().gap(2.), |col, path| {
            let label = if path == Path::new("/") {
                "Computer".to_string()
            } else {
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            };
            let glyph = if path == Path::new("/") { "laptop" } else { "hard-drive" };
            col.child(sidebar_button(
                app,
                style,
                glyph,
                &label,
                Location::Folder(path),
            ))
        });
    let tags = model::TAG_COLORS
        .iter()
        .fold(column().gap(1.), |col, color| {
            col.child(tag_sidebar_button(app, style, color))
        });
    // macOS 27: the sidebar is a floating glass card inset from the window
    // edge, the traffic lights at its top; the content runs edge to edge
    // behind it.
    let _ = style.sidebar_bg();
    column()
        .w(214.)
        .h_full()
        .shrink_0()
        .m(8.)
        .mr(0.)
        .px(10.)
        .pt(14.)
        .pb(10.)
        .gap(4.)
        .rounded(14.)
        .bg(if style.dark() { Color(255, 255, 255, 16) } else { Color(255, 255, 255, 120) })
        .border(1.)
        .border_color(if style.dark() { Color(255, 255, 255, 26) } else { Color(0, 0, 0, 18) })
        .window_drag()
        .child(row().pl(4.).pb(14.).child(lights))
        .child(sidebar_section("Favourites", style, favourites))
        .child(sidebar_section("Locations", style, locations))
        .child(sidebar_section("Tags", style, tags))
}

fn folder_icon(style: &solyra_ui::Style) -> View {
    // The same folder artwork as the dock (Solyra's own, bundled in solyra-ui).
    if let Some(img) = solyra_ui::icons::get("folder") {
        return image("Folder", img).w(64.).h(64.);
    }
    let folder = rgb(0x53a7f3);
    let folder_edge = rgb(0x2578c8);
    let paper = if style.dark() {
        rgb(0xdbdee7)
    } else {
        rgb(0xffffff)
    };
    column()
        .w(56.)
        .h(46.)
        .items_start()
        .child(
            row()
                .h(9.)
                .items_end()
                .child(div().w(22.).h(7.).ml(4.).rounded(4.).bg(folder_edge)),
        )
        .child(
            div()
                .w(56.)
                .h(36.)
                .rounded(7.)
                .bg(folder)
                .border(1.)
                .border_color(folder_edge)
                .child(
                    div()
                        .w(46.)
                        .h(2.)
                        .ml(5.)
                        .mt(6.)
                        .rounded(1.)
                        .bg(paper)
                        .opacity(0.45),
                ),
        )
}

fn file_icon(style: &solyra_ui::Style) -> View {
    let paper = if style.dark() {
        rgb(0xdfe2e9)
    } else {
        rgb(0xffffff)
    };
    let ink = if style.dark() {
        rgb(0x777d8d)
    } else {
        rgb(0xb8bfca)
    };
    div()
        .w(43.)
        .h(52.)
        .mt(2.)
        .rounded(6.)
        .bg(paper)
        .border(1.)
        .border_color(ink)
        .child(
            column()
                .gap(4.)
                .p(8.)
                .pt(14.)
                .child(div().w(23.).h(2.).rounded(1.).bg(ink))
                .child(div().w(23.).h(2.).rounded(1.).bg(ink))
                .child(div().w(16.).h(2.).rounded(1.).bg(ink)),
        )
}

fn item_icon(entry: &Entry, style: &solyra_ui::Style) -> View {
    if entry.is_dir {
        folder_icon(style)
    } else {
        file_icon(style)
    }
}

fn select_background(selected: bool, theme: solyra_tokens::Theme) -> Styles {
    if selected {
        Styles::new().bg(solyra_ui::color(theme.selected_fill))
    } else {
        Styles::new().bg(Color(0, 0, 0, 0))
    }
}

fn grid_item(app: &FilesApp, style: &solyra_ui::Style, entry: Entry) -> View {
    let selected = {
        let (selected, path) = (app.selected.clone(), entry.path.clone());
        move || selected.get().as_ref() == Some(&path)
    };
    let activate = {
        let (app, entry) = (app.clone(), entry.clone());
        move |cx: &mut zgx::input::EventContext| {
            if cx.phase == EventPhase::Capture {
                return;
            }
            if let InputEvent::PointerDown {
                button: PointerButton::Primary,
                ..
            } = &cx.event
            {
                app.tap_entry(&entry);
                cx.prevent_default();
            }
        }
    };
    let theme = style.theme;
    div()
        .w(88.)
        .h(98.)
        .p(6.)
        .rounded(10.)
        .reactive_style(move || select_background(selected(), theme))
        .on_event(activate)
        .child(
            column()
                .w_full()
                .h_full()
                .gap(7.)
                .items_center()
                .child(item_icon(&entry, style))
                .child(
                    text(entry.name.clone())
                        .text_size(11.5)
                        .text_align(zgx::text_layout::TextAlign::Center)
                        .line_clamp(2)
                        .truncate()
                        .w_full(),
                ),
        )
}

fn list_item(app: &FilesApp, style: &solyra_ui::Style, entry: Entry, tag: Option<String>) -> View {
    let selected = {
        let (selected, path) = (app.selected.clone(), entry.path.clone());
        move || selected.get().as_ref() == Some(&path)
    };
    let activate = {
        let (app, entry) = (app.clone(), entry.clone());
        move |cx: &mut zgx::input::EventContext| {
            if cx.phase == EventPhase::Capture {
                return;
            }
            if let InputEvent::PointerDown {
                button: PointerButton::Primary,
                ..
            } = &cx.event
            {
                app.tap_entry(&entry);
                cx.prevent_default();
            }
        }
    };
    let theme = style.theme;
    let kind = if entry.is_dir { "Folder" } else { "File" };
    let size = if entry.is_dir {
        "—".into()
    } else {
        model::format_size(entry.size)
    };
    div()
        .w_full()
        .h(34.)
        .px(12.)
        .rounded(7.)
        .reactive_style(move || select_background(selected(), theme))
        .on_event(activate)
        .child(
            row()
                .w_full()
                .h_full()
                .gap(10.)
                .items_center()
                .child(
                    text(if entry.is_dir { "▰" } else { "▱" }.to_string())
                        .text_color(rgb(0x53a7f3))
                        .w(20.),
                )
                .child(text(entry.name.clone()).text_size(12.5).truncate().grow())
                .child(
                    div()
                        .w(8.)
                        .h(8.)
                        .rounded(4.)
                        .bg(tag.as_deref().map(tag_color).unwrap_or(Color(0, 0, 0, 0))),
                )
                .child(
                    text(size)
                        .text_size(11.5)
                        .text_color(solyra_ui::color(theme.text_dim))
                        .w(72.),
                )
                .child(
                    text(kind)
                        .text_size(11.5)
                        .text_color(solyra_ui::color(theme.text_dim))
                        .w(58.),
                ),
        )
}

fn list_header(style: &solyra_ui::Style) -> View {
    let theme = style.theme;
    row()
        .w_full()
        .h(25.)
        .px(12.)
        .gap(10.)
        .items_center()
        .border_edges(zgx::scene::Insets {
            top: 0.,
            right: 0.,
            bottom: 1.,
            left: 0.,
        })
        .border_color(style.hairline())
        .child(div().w(20.))
        .child(
            text("Name")
                .text_size(11.)
                .font_bold()
                .text_color(solyra_ui::color(theme.text_dim))
                .grow(),
        )
        .child(div().w(8.))
        .child(
            text("Size")
                .text_size(11.)
                .font_bold()
                .text_color(solyra_ui::color(theme.text_dim))
                .w(72.),
        )
        .child(
            text("Kind")
                .text_size(11.)
                .font_bold()
                .text_color(solyra_ui::color(theme.text_dim))
                .w(58.),
        )
}

fn empty_state(message: &str, style: &solyra_ui::Style) -> View {
    column()
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .gap(8.)
        .child(
            text("◇".to_string())
                .text_size(34.)
                .text_color(solyra_ui::color(style.theme.text_dim)),
        )
        .child(
            text(message.to_string())
                .text_size(13.)
                .text_color(solyra_ui::color(style.theme.text_dim)),
        )
}

fn entries_view(
    app: &FilesApp,
    style: &solyra_ui::Style,
    entries: Vec<Entry>,
    cx: &mut zgx::compose::Context,
) -> View {
    let query = app.query.get();
    let entries = model::filtered(&entries, &query);
    if entries.is_empty() {
        let message = if query.trim().is_empty() {
            "This folder is empty."
        } else {
            "Nothing matches this search."
        };
        return empty_state(message, style);
    }
    let offset = cx.runtime().signal(0f32);
    if app.mode.get() == 0 {
        let cards = entries
            .into_iter()
            .map(|entry| grid_item(app, style, entry))
            .collect::<Vec<_>>();
        scroll(offset)
            .grow()
            .w_full()
            .child(row().flex_wrap().gap(10.).p(18.).w_full().children(cards))
    } else if app.mode.get() == 1 {
        let tags: HashMap<PathBuf, String> = model::tags().into_iter().collect();
        let mut rows = vec![list_header(style)];
        rows.extend(entries.into_iter().map(|entry| {
            let tag = tags.get(&entry.path).cloned();
            list_item(app, style, entry, tag)
        }));
        scroll(offset)
            .grow()
            .w_full()
            .child(column().gap(2.).p(12.).pt(0.).w_full().children(rows))
    } else {
        // Column view (macOS Finder Miller column list with folder chevrons).
        let theme = style.theme;
        let rows = entries
            .into_iter()
            .map(|entry| {
                let selected = {
                    let (selected, path) = (app.selected.clone(), entry.path.clone());
                    move || selected.get().as_ref() == Some(&path)
                };
                let activate = {
                    let (app, entry) = (app.clone(), entry.clone());
                    move |cx: &mut zgx::input::EventContext| {
                        if cx.phase == EventPhase::Capture {
                            return;
                        }
                        if let InputEvent::PointerDown {
                            button: PointerButton::Primary,
                            ..
                        } = &cx.event
                        {
                            app.tap_entry(&entry);
                            cx.prevent_default();
                        }
                    }
                };
                let chevron = if entry.is_dir { "›" } else { "" };
                div()
                    .w_full()
                    .h(30.)
                    .px(10.)
                    .rounded(6.)
                    .reactive_style(move || select_background(selected(), theme))
                    .on_event(activate)
                    .child(
                        row()
                            .w_full()
                            .h_full()
                            .gap(8.)
                            .items_center()
                            .child(
                                text(if entry.is_dir { "▰" } else { "▱" }.to_string())
                                    .text_color(rgb(0x53a7f3))
                                    .w(16.),
                            )
                            .child(text(entry.name.clone()).text_size(12.5).truncate().grow())
                            .child(
                                text(chevron.to_string())
                                    .text_size(13.)
                                    .text_color(solyra_ui::color(theme.text_dim)),
                            ),
                    )
            })
            .collect::<Vec<_>>();
        scroll(offset)
            .grow()
            .w_full()
            .child(column().gap(1.).p(10.).w_full().children(rows))
    }
}

fn action_button(label: &str, on_click: impl FnMut() + 'static) -> View {
    button()
        .h(30.)
        .px(11.)
        .rounded(7.)
        .items_center()
        .border(0.)
        .child(text(label.to_string()).text_size(12.))
        .on_click(on_click)
}

fn selection_disabled(app: &FilesApp) -> impl FnMut() -> bool + 'static {
    let selected = app.selected.clone();
    move || selected.get().is_none()
}

fn action_bar_for(app: &FilesApp, style: &solyra_ui::Style, location: &Location) -> View {
    let in_trash = location == &Location::Trash;
    let open = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Open", move || {
            let Some(path) = app.selected.get() else {
                return;
            };
            if let Some(entry) = entry_for_path(&path) {
                app.open_entry(&entry);
            }
        })
        .disabled_when(disabled)
    };
    let rename = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Rename", move || {
            let Some(path) = app.selected.get() else {
                return;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            app.rename_text.set(name);
            app.renaming.set(Some(path));
        })
        .disabled_when(disabled)
    };
    let star = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Star", move || {
            let Some(path) = app.selected.get() else {
                return;
            };
            match model::toggle_star(&path) {
                Ok(true) => {
                    app.status.set("Added to Starred.".into());
                }
                Ok(false) => {
                    app.status.set("Removed from Starred.".into());
                }
                Err(e) => {
                    app.status.set(format!("Could not update Starred: {e}"));
                }
            };
            app.refresh();
        })
        .disabled_when(disabled)
    };
    let tag = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Tag", move || {
            let Some(path) = app.selected.get() else {
                return;
            };
            match model::cycle_tag(&path) {
                Ok(Some(color)) => {
                    app.status.set(format!("Tagged {color}."));
                }
                Ok(None) => {
                    app.status.set("Tag removed.".into());
                }
                Err(e) => {
                    app.status.set(format!("Could not update the tag: {e}"));
                }
            };
            app.refresh();
        })
        .disabled_when(disabled)
    };
    let copy = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Copy", move || {
            let Some(path) = app.selected.get() else {
                return;
            };
            *app.clipboard.borrow_mut() = Some((path, false));
            app.status.set("Copied.".into());
            app.refresh();
        })
        .disabled_when(disabled)
    };
    let cut = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Cut", move || {
            let Some(path) = app.selected.get() else {
                return;
            };
            *app.clipboard.borrow_mut() = Some((path, true));
            app.status
                .set("Cut. Paste it somewhere else to move it.".into());
            app.refresh();
        })
        .disabled_when(disabled)
    };
    let paste = {
        let paste_disabled = {
            let app = app.clone();
            move || {
                let _ = app.revision.get();
                app.clipboard.borrow().is_none() || location_dir(&app.location.get()).is_none()
            }
        };
        let app = app.clone();
        action_button("Paste", move || {
            let item = app.clipboard.borrow().clone();
            let Some((source, cut)) = item else {
                return;
            };
            let Some(dir) = location_dir(&app.location.get()) else {
                return;
            };
            match model::paste_entry(&source, &dir, cut) {
                Ok(dest) => {
                    if cut {
                        *app.clipboard.borrow_mut() = None;
                    }
                    app.status.set(format!("Pasted as {}.", dest.display()));
                    app.selected.set(Some(dest));
                }
                Err(e) => {
                    app.status.set(e);
                }
            };
            app.refresh();
        })
        .disabled_when(paste_disabled)
    };
    let new_folder = {
        let disabled = location_dir(location).is_none();
        let app = app.clone();
        action_button("New Folder", move || {
            let Some(dir) = location_dir(&app.location.get()) else {
                return;
            };
            match model::create_folder(&dir) {
                Ok(path) => {
                    app.status.set("Created a new folder.".into());
                    app.rename_text.set("untitled folder".into());
                    app.renaming.set(Some(path.clone()));
                    app.selected.set(Some(path));
                }
                Err(e) => {
                    app.status.set(e);
                }
            };
            app.refresh();
        })
        .disabled_when(move || disabled)
    };
    let trash = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Move to Trash", move || {
            if let Some(path) = app.selected.get() {
                app.confirm_trash.set(Some(path));
            }
        })
        .disabled_when(disabled)
    };
    let restore = {
        let disabled = selection_disabled(&app);
        let app = app.clone();
        action_button("Restore", move || {
            let Some(path) = app.selected.get() else {
                return;
            };
            match model::restore_from_trash(&path) {
                Ok(dest) => {
                    app.status.set(format!("Restored to {}.", dest.display()));
                }
                Err(e) => {
                    app.status.set(e);
                }
            };
            app.selected.set(None);
            app.refresh();
        })
        .disabled_when(disabled)
    };
    let mut controls = row()
        .gap(6.)
        .child(new_folder)
        .child(open)
        .child(rename)
        .child(star)
        .child(tag)
        .child(copy)
        .child(cut)
        .child(paste);
    controls = if in_trash {
        controls.child(restore)
    } else {
        controls.child(trash)
    };
    row()
        .w_full()
        .h(42.)
        .px(12.)
        .gap(10.)
        .items_center()
        .border_edges(zgx::scene::Insets {
            top: 1.,
            right: 0.,
            bottom: 0.,
            left: 0.,
        })
        .border_color(style.hairline())
        .child(controls)
        .child(
            text_signal({
                let status = app.status.clone();
                move || status.get()
            })
            .text_size(11.5)
            .text_color(solyra_ui::color(style.theme.text_dim))
            .truncate()
            .grow(),
        )
}

fn action_bar(app: &FilesApp, style: &solyra_ui::Style) -> View {
    let app = app.clone();
    let style = *style;
    switch(
        {
            let location = app.location.clone();
            move || location.get()
        },
        move |location, _cx| action_bar_for(&app, &style, &location),
    )
}

fn confirm_bar(app: &FilesApp, style: &solyra_ui::Style) -> View {
    let app = app.clone();
    let style = *style;
    switch(
        {
            let confirm = app.confirm_trash.clone();
            move || confirm.get()
        },
        move |path, _cx| {
            let Some(path) = path else {
                return column().h(0.);
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "this item".into());
            let cancel = {
                let app = app.clone();
                small_button("Cancel", move || {
                    app.confirm_trash.set(None);
                })
            };
            let delete = {
                let app = app.clone();
                let path = path.clone();
                button()
                    .h(28.)
                    .px(10.)
                    .rounded(7.)
                    .items_center()
                    .border(0.)
                    .bg(solyra_ui::color(style.theme.danger))
                    .text_color(solyra_ui::color(style.theme.on_accent))
                    .child(text("Move to Trash").text_size(12.))
                    .on_click(move || {
                        match model::move_to_trash(&path) {
                            Ok(_) => {
                                app.status.set("Moved to Trash.".into());
                            }
                            Err(e) => {
                                app.status.set(e);
                            }
                        };
                        app.confirm_trash.set(None);
                        app.selected.set(None);
                        app.refresh();
                    })
            };
            row()
                .w_full()
                .h(42.)
                .px(14.)
                .gap(10.)
                .items_center()
                .bg(solyra_ui::color(style.theme.warning))
                .child(
                    text(format!("Move {name} to the Trash?"))
                        .text_size(12.5)
                        .font_bold()
                        .grow(),
                )
                .child(cancel)
                .child(delete)
        },
    )
}

fn rename_bar(app: &FilesApp, style: &solyra_ui::Style) -> View {
    let app = app.clone();
    let style = *style;
    switch(
        {
            let renaming = app.renaming.clone();
            move || renaming.get()
        },
        move |path, _cx| {
            let Some(path) = path else {
                return column().h(0.);
            };
            let input = text_input("New name", app.rename_text.clone())
                .w(280.)
                .h(30.)
                .px(9.)
                .rounded(7.)
                .bg(solyra_ui::color(style.theme.field));
            let cancel = {
                let app = app.clone();
                small_button("Cancel", move || {
                    app.renaming.set(None);
                })
            };
            let save = {
                let app = app.clone();
                let path = path.clone();
                small_button("Rename", move || {
                    match model::rename_entry(&path, &app.rename_text.get()) {
                        Ok(dest) => {
                            app.status.set(format!("Renamed to {}.", dest.display()));
                            app.selected.set(Some(dest));
                        }
                        Err(e) => {
                            app.status.set(e);
                        }
                    }
                    app.renaming.set(None);
                    app.refresh();
                })
            };
            row()
                .w_full()
                .h(46.)
                .px(14.)
                .gap(10.)
                .items_center()
                .bg(style.sidebar_bg())
                .child(text("New name").text_size(12.5))
                .child(input)
                .child(save)
                .child(cancel)
        },
    )
}

fn content(app: &FilesApp, style: &solyra_ui::Style) -> View {
    let app = app.clone();
    let style = *style;
    switch(
        {
            let app = app.clone();
            move || {
                (
                    app.location.get(),
                    app.revision.get(),
                    app.mode.get(),
                    app.query.get(),
                    app.selected.get(),
                    app.show_hidden.get(),
                )
            }
        },
        move |key, cx| {
            let (location, _, _, _, _, show_hidden) = key;
            match load_entries(&location, show_hidden) {
                Ok(entries) => entries_view(&app, &style, entries, cx),
                Err(e) => empty_state(&e, &style),
            }
        },
    )
    .grow()
    .w_full()
}

fn breadcrumbs_for(app: &FilesApp, style: &solyra_ui::Style, location: Location) -> View {
    let mut crumbs = row().gap(4.).items_center();
    if let Location::Folder(path) = &location {
        let home = model::home_dir();
        if let Ok(rel) = path.strip_prefix(&home) {
            let home_button = {
                let app = app.clone();
                let home = home.clone();
                small_button("Home", move || app.navigate(Location::Folder(home.clone())))
            };
            crumbs = crumbs.child(home_button);
            let mut cur = home.clone();
            for part in rel.components() {
                cur.push(part.as_os_str());
                let label = part.as_os_str().to_string_lossy().into_owned();
                let (app, target) = (app.clone(), cur.clone());
                crumbs = crumbs
                    .child(text("›").text_color(solyra_ui::color(style.theme.text_dim)))
                    .child(small_button(&label, move || {
                        app.navigate(Location::Folder(target.clone()))
                    }));
            }
        } else {
            crumbs = crumbs.child(
                text(path.display().to_string())
                    .text_size(12.)
                    .text_color(solyra_ui::color(style.theme.text_dim)),
            );
        }
    } else {
        crumbs = crumbs.child(
            text(location.title())
                .text_size(12.)
                .text_color(solyra_ui::color(style.theme.text_dim)),
        );
    }
    crumbs
        .w_full()
        .h(34.)
        .px(14.)
        .bg(Color(0, 0, 0, 0))
        .border_edges(zgx::scene::Insets {
            top: 0.,
            right: 0.,
            bottom: 1.,
            left: 0.,
        })
        .border_color(style.hairline())
}

fn breadcrumbs(app: &FilesApp, style: &solyra_ui::Style) -> View {
    let app = app.clone();
    let style = *style;
    switch(
        {
            let location = app.location.clone();
            move || location.get()
        },
        move |location, _cx| breadcrumbs_for(&app, &style, location),
    )
}

fn start_location() -> Location {
    let arg = std::env::args().skip(1).find(|arg| !arg.starts_with("--"));
    match arg.as_deref() {
        Some("trash:///") => Location::Trash,
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_dir() {
                Location::Folder(path)
            } else {
                Location::Home
            }
        }
        None => Location::Home,
    }
}

fn send_to_notch_shelf(path: &Path) -> Result<(), String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| "HOME is not set".to_string())?;
    let shelf = base.join("solyra/shelf.list");
    if let Some(dir) = shelf.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut lines: Vec<String> = fs::read_to_string(&shelf)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let p_str = path.display().to_string();
    lines.retain(|l| l != &p_str);
    lines.insert(0, p_str);
    lines.truncate(8);
    fs::write(&shelf, format!("{}\n", lines.join("\n"))).map_err(|e| e.to_string())?;
    Ok(())
}

fn inspector_pane(app: &FilesApp, style: &solyra_ui::Style) -> View {
    let app = app.clone();
    let style = *style;
    switch(
        {
            let app = app.clone();
            move || (app.show_inspector.get(), app.selected.get())
        },
        move |(open, selected), _cx| {
            if !open {
                return div();
            }
            let theme = style.theme;
            let body = if let Some(entry) = selected.as_deref().and_then(entry_for_path) {
                let kind = if entry.is_dir { "Folder" } else { "File" };
                let size = if entry.is_dir {
                    "—".to_string()
                } else {
                    model::format_size(entry.size)
                };
                let shelf_btn = {
                    let app = app.clone();
                    let path = entry.path.clone();
                    small_button("Send to Notch Shelf", move || {
                        let msg = match send_to_notch_shelf(&path) {
                            Ok(()) => "Parked in Notch Shelf.".to_string(),
                            Err(e) => format!("Could not park file: {e}"),
                        };
                        app.status.set(msg);
                    })
                };
                column()
                    .gap(10.)
                    .items_center()
                    .child(item_icon(&entry, &style))
                    .child(text(entry.name.clone()).font_bold().text_size(13.).truncate())
                    .child(
                        text(format!("{kind} · {size}"))
                            .text_size(11.5)
                            .text_color(solyra_ui::color(theme.text_dim)),
                    )
                    .child(
                        text(entry.path.display().to_string())
                            .text_size(11.)
                            .text_color(solyra_ui::color(theme.text_dim))
                            .line_clamp(3)
                            .truncate()
                            .w_full(),
                    )
                    .child(shelf_btn)
            } else {
                column()
                    .gap(6.)
                    .items_center()
                    .child(
                        text("Quick Look")
                            .font_bold()
                            .text_size(13.)
                            .text_color(solyra_ui::color(theme.text)),
                    )
                    .child(
                        text("Select a file or folder to inspect.")
                            .text_size(12.)
                            .text_color(solyra_ui::color(theme.text_dim)),
                    )
            };
            column()
                .w(230.)
                .h_full()
                .shrink_0()
                .p(16.)
                .bg(style.sidebar_bg())
                .border_edges(zgx::scene::Insets {
                    top: 0.,
                    right: 0.,
                    bottom: 0.,
                    left: 1.,
                })
                .border_color(style.hairline())
                .child(body)
        },
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let self_test = std::env::args().any(|arg| arg == "--self-test");
    let style = solyra_ui::current();
    let theme = style.theme;
    Application::new()
        .window(WindowOptions {
            title: "Files".into(),
            width: 1020.,
            height: 660.,
            min_size: Some((680., 420.)),
            transparent: style.glass && cfg!(target_os = "linux"),
            decorations: false,
            ..Default::default()
        })
        .run(move |cx| {
            let rt = cx.ui.runtime.clone();
            let app = FilesApp {
                location: rt.signal(start_location()),
                revision: rt.signal(0),
                selected: rt.signal(None),
                query: rt.signal(String::new()),
                mode: rt.signal(0),
                show_hidden: rt.signal(false),
                show_inspector: rt.signal(false),
                confirm_trash: rt.signal(None),
                renaming: rt.signal(None),
                rename_text: rt.signal(String::new()),
                status: rt.signal(String::new()),
                back: Rc::default(),
                forward: Rc::default(),
                last_click: Rc::default(),
                clipboard: Rc::default(),
            };
            let window = cx.window.clone();

            // One frosted surface: the compositor blurs behind the window,
            // this single tint lies under sidebar, toolbar and content.
            let surface = if style.glass {
                if style.dark() { Color(24, 26, 30, 168) } else { Color(246, 246, 248, 190) }
            } else {
                style.content_bg()
            };
            let body = row()
                .w_full()
                .h_full()
                .rounded(16.)
                .overflow_hidden()
                .border(1.)
                .border_color(Color(255, 255, 255, 30))
                .text_color(solyra_ui::color(theme.text))
                .bg(surface)
                .child(sidebar(&app, &style, window_lights(window.clone())))
                .child(
                    column()
                        .grow()
                        .h_full()
                        .child(toolbar(&app, &style, window))
                        .child(rename_bar(&app, &style))
                        .child(confirm_bar(&app, &style))
                        .child(breadcrumbs(&app, &style))
                        .child(div().grow().w_full().overflow_hidden().child(content(&app, &style)))
                        .child(div().w_full().shrink_0().child(action_bar(&app, &style))),
                )
                .child(inspector_pane(&app, &style));

            cx.render(solyra_ui::scope(&style, body));

            if self_test {
                let window = cx.window.clone();
                cx.tasks.spawn(async move {
                    zgx::timer::sleep(Duration::from_secs(2)).await;
                    window.close();
                });
            }
        })
}

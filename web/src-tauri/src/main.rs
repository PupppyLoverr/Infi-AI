//! solyra-web: Solyra's shell surfaces and apps rendered with React in
//! Tauri webviews; Rust does everything system-side.
//!
//! - `solyra-web` / `solyra-web shell`: dock, Dynamic Island, Spotlight and
//!   Control Centre as layer-shell surfaces on solyra-comp.
//! - `solyra-web files [path]`: the Finder-style file manager window.
//! - `solyra-web terminal`: the terminal (tabs, splits, agents sidebar,
//!   chat view for agent harnesses).
//! - `solyra-web toggle spotlight|control|island`: for key bindings.
//! - `solyra-web emit <surface> <event> [text]`: e.g. `emit island island-open`,
//!   `emit spotlight spotlight-query notes`.

mod glass;
mod surface;
mod sys;
mod term;

#[allow(dead_code)]
#[path = "../../../shell/panel/src/media.rs"]
mod media;
#[allow(dead_code)]
#[path = "../../../shell/panel/src/glass_proto.rs"]
mod glass_proto;
#[allow(dead_code)]
#[path = "../../../shell/panel/src/shelf.rs"]
mod shelf;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("solyra-web.sock")
}

struct MediaSlot(Arc<Mutex<Option<media::Media>>>);

#[derive(Serialize, Clone)]
struct MediaInfo {
    title: String,
    artist: String,
    playing: bool,
    position_us: Option<i64>,
    length_us: Option<i64>,
}

#[derive(Serialize, Clone)]
struct Shown {
    label: String,
    shown: bool,
}

// ------------------------------------------------------------ commands

#[tauri::command]
fn surface_resize(window: tauri::WebviewWindow, width: i32, height: i32) {
    surface::resize(window.label(), width, height);
}

#[tauri::command]
fn surface_input(window: tauri::WebviewWindow, rects: Vec<[i32; 4]>) {
    surface::input_rects(window.label(), &rects);
}

#[tauri::command]
fn surface_glass(window: tauri::WebviewWindow, boxes: Vec<[i32; 5]>, preset: String) {
    surface::glass(window.label(), &boxes, &preset);
}

#[tauri::command]
fn surface_hide(window: tauri::WebviewWindow) {
    surface::show(window.label(), false);
}

#[tauri::command]
fn surface_toggle(app: AppHandle, label: String) {
    toggle_surface(&app, &label);
}

#[tauri::command]
fn apps() -> Vec<sys::App> {
    sys::apps()
}

#[tauri::command]
fn dock_pins() -> Vec<sys::Pin> {
    sys::dock_pins()
}

#[tauri::command]
fn launch(cmd: String) {
    sys::launch(&cmd);
}

#[tauri::command]
fn focus_window(id: u64) {
    sys::focus_window(id);
}

#[tauri::command]
fn status() -> sys::Status {
    sys::status()
}

#[tauri::command]
fn set_volume(pct: u8) {
    sys::set_volume(pct);
}

#[tauri::command]
fn set_brightness(pct: u8) {
    sys::set_brightness(pct);
}

#[tauri::command]
fn media(slot: tauri::State<'_, MediaSlot>) -> Option<MediaInfo> {
    slot.0.lock().ok()?.as_ref().map(|m| MediaInfo {
        title: m.title.clone(),
        artist: m.artist.clone(),
        playing: m.playing,
        position_us: m.position_us,
        length_us: m.length_us,
    })
}

#[tauri::command]
fn media_control(slot: tauri::State<'_, MediaSlot>, action: String, position_us: Option<i64>) {
    let Some(bus) = slot.0.lock().ok().and_then(|m| m.as_ref().map(|m| m.bus.clone())) else { return };
    let c = match action.as_str() {
        "play-pause" => media::Control::PlayPause,
        "next" => media::Control::Next,
        "previous" => media::Control::Previous,
        "seek" => media::Control::SeekTo(position_us.unwrap_or(0)),
        _ => return,
    };
    media::control(bus, c);
}

#[tauri::command]
fn clipboard() -> Vec<sys::Clip> {
    sys::clipboard()
}

#[tauri::command]
fn copy_text(text: String) {
    sys::copy_text(&text);
}

#[tauri::command]
fn shelf() -> Vec<sys::ShelfItem> {
    sys::shelf()
}

#[tauri::command]
fn shelf_add(path: String) {
    sys::shelf_add(&path);
}

#[tauri::command]
fn shelf_remove(path: String) {
    sys::shelf_remove(&path);
}

#[tauri::command]
fn list_dir(path: String, hidden: bool) -> Result<Vec<sys::Entry>, String> {
    sys::list_dir(&path, hidden)
}

#[tauri::command]
fn open_path(path: String) {
    sys::open_path(&path);
}

#[tauri::command]
fn home_dir() -> String {
    sys::home_dir()
}

#[tauri::command]
fn find_files(query: String) -> Vec<sys::Entry> {
    sys::find_files(&query, 6)
}

#[tauri::command]
fn pty_spawn(app: AppHandle, ptys: tauri::State<'_, term::Ptys>, id: u32, cmd: Option<String>, cwd: Option<String>, cols: u16, rows: u16) -> Result<(), String> {
    ptys.spawn(app, id, cmd, cwd, cols, rows)
}

#[tauri::command]
fn pty_write(ptys: tauri::State<'_, term::Ptys>, id: u32, data: String) {
    ptys.write(id, &data);
}

#[tauri::command]
fn pty_resize(ptys: tauri::State<'_, term::Ptys>, id: u32, cols: u16, rows: u16) {
    ptys.resize(id, cols, rows);
}

#[tauri::command]
fn pty_kill(ptys: tauri::State<'_, term::Ptys>, id: u32) {
    ptys.kill(id);
}

#[tauri::command]
fn harnesses() -> Vec<&'static str> {
    term::harnesses()
}

#[tauri::command]
fn agent_send(app: AppHandle, agents: tauri::State<'_, term::Agents>, key: String, harness: String, prompt: String, opts: term::AgentOpts) -> Result<(), String> {
    term::agent_send(app, &agents, key, harness, prompt, opts)
}

#[tauri::command]
fn agent_stop(agents: tauri::State<'_, term::Agents>, key: String) {
    agents.stop(&key);
}

#[tauri::command]
fn window_action(window: tauri::WebviewWindow, action: String) {
    let _ = match action.as_str() {
        "close" => window.close(),
        "minimize" => window.minimize(),
        "maximize" => window.is_maximized().and_then(|m| if m { window.unmaximize() } else { window.maximize() }),
        "drag" => window.start_dragging(),
        _ => Ok(()),
    };
}

// ------------------------------------------------------------ IPC

fn toggle_surface(app: &AppHandle, label: &str) {
    let shown = surface::toggle(label);
    let _ = app.emit_to(label, "surface-shown", Shown { label: label.into(), shown });
}

fn serve_ipc(app: AppHandle) {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        eprintln!("solyra-web: cannot listen on {}", path.display());
        return;
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let mut words = line.split_whitespace();
            let (verb, label, event) = (words.next(), words.next(), words.next());
            let payload = words.collect::<Vec<_>>().join(" ");
            match (verb, label, event) {
                (Some("toggle"), Some(label), _) => {
                    let label = label.to_string();
                    let app2 = app.clone();
                    let _ = app.run_on_main_thread(move || toggle_surface(&app2, &label));
                }
                // `emit island island-open`: poke a surface (key bindings, tests).
                (Some("emit"), Some(label), Some(event)) => {
                    let _ = app.emit_to(label, event, payload);
                }
                _ => {}
            }
        }
    });
}

fn send_ipc(msg: &str) -> std::io::Result<()> {
    let mut s = UnixStream::connect(socket_path())?;
    s.write_all(msg.as_bytes())?;
    s.write_all(b"\n")
}

// ------------------------------------------------------------ main

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("shell").to_string();
    if mode == "toggle" || mode == "emit" {
        let rest = args[2..].join(" ");
        let msg = if rest.is_empty() { format!("{mode} spotlight") } else { format!("{mode} {rest}") };
        if let Err(e) = send_ipc(&msg) {
            eprintln!("solyra-web {mode}: shell not running ({e})");
            std::process::exit(1);
        }
        return;
    }
    let extra = args.get(2).cloned();
    let media_slot: Arc<Mutex<Option<media::Media>>> = Arc::default();
    if mode == "shell" {
        media::spawn_watcher(media_slot.clone(), || {});
    }

    tauri::Builder::default()
        .manage(MediaSlot(media_slot))
        .manage(term::Ptys::default())
        .manage(term::Agents::default())
        .invoke_handler(tauri::generate_handler![
            surface_resize, surface_input, surface_glass, surface_hide, surface_toggle, apps, dock_pins, launch,
            focus_window, status, set_volume, set_brightness, media, media_control, clipboard,
            copy_text, shelf, shelf_add, shelf_remove, list_dir, open_path, home_dir, find_files,
            pty_spawn, pty_write, pty_resize, pty_kill, harnesses, agent_send, agent_stop, window_action
        ])
        .setup(move |app| {
            match mode.as_str() {
                "files" | "terminal" => {
                    let (title, w, h) = if mode == "files" { ("Files", 1080.0, 680.0) } else { ("Terminal", 1100.0, 700.0) };
                    let mut url = format!("index.html#{mode}");
                    if let Some(p) = &extra {
                        url.push_str(&format!("?path={}", p.replace(' ', "%20")));
                    }
                    WebviewWindowBuilder::new(app, mode.as_str(), WebviewUrl::App(url.into()))
                        .title(title)
                        .transparent(true)
                        .decorations(false)
                        .inner_size(w, h)
                        .build()?;
                }
                _ => {
                    for spec in surface::SPECS {
                        let win = WebviewWindowBuilder::new(
                            app,
                            spec.label,
                            WebviewUrl::App(format!("index.html#{}", spec.label).into()),
                        )
                        .transparent(true)
                        .decorations(false)
                        .visible(false)
                        .inner_size(spec.width as f64, spec.height as f64)
                        .build()?;
                        surface::adopt(&win, *spec)?;
                    }
                    serve_ipc(app.handle().clone());
                    let _ = app.get_webview_window("dock");
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("solyra-web failed");
}

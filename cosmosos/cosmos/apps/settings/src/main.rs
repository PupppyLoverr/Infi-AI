//! cosmos-settings — real settings for CosmosOS.
//!
//! Every control maps to a key in ~/.config/cosmos/config.toml (the file the
//! compositor and shell read) AND pushes the change live over cosmos-ipc
//! SetConfig so the running compositor/shell pick it up without a restart.
//! Settings that don't exist aren't shown.

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use cosmos_ipc::{read_message, write_message, Event, Request};

#[derive(Clone)]
struct Cfg {
    appearance: String,   // "dark" | "light"
    reduce_motion: bool,
    scale: f64,
    terminal: String,
    launcher_rows: u32,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            appearance: "dark".into(),
            reduce_motion: false,
            scale: 1.0,
            terminal: "cosmos-terminal".into(),
            launcher_rows: 10,
        }
    }
}

fn config_path() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())).join(".config")
        })
        .join("cosmos/config.toml")
}

fn load() -> Cfg {
    let mut c = Cfg::default();
    let Ok(text) = std::fs::read_to_string(config_path()) else {
        return c;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"');
        match k.trim() {
            "appearance" => c.appearance = v.to_string(),
            "reduce_motion" => c.reduce_motion = v == "true",
            "scale" => c.scale = v.parse().unwrap_or(1.0),
            "terminal" => c.terminal = v.to_string(),
            "launcher_rows" => c.launcher_rows = v.parse().unwrap_or(10),
            _ => {}
        }
    }
    c
}

fn save(c: &Cfg) -> std::io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = format!(
        "# CosmosOS configuration\nappearance = \"{}\"\nreduce_motion = {}\nscale = {}\nterminal = \"{}\"\nlauncher_rows = {}\n",
        c.appearance, c.reduce_motion, c.scale, c.terminal, c.launcher_rows
    );
    std::fs::write(path, text)
}

/// Push one key live to the compositor (which applies + rebroadcasts it).
fn push(key: &str, value: serde_json::Value) -> Result<(), String> {
    let path = cosmos_ipc::socket_path();
    let mut stream =
        UnixStream::connect(&path).map_err(|e| format!("ipc connect {}: {e}", path.display()))?;
    write_message(
        &mut stream,
        &Request::SetConfig {
            key: key.to_string(),
            value,
        },
    )
    .map_err(|e| format!("ipc write: {e}"))?;
    // Read until the compositor acks (or errors). Unrelated events are
    // ignored — a subscribe-side broadcast is not an ack.
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .ok();
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    for _ in 0..8 {
        match read_message::<Event, _>(&mut reader) {
            Ok(Some(Event::Error { message })) => {
                return Err(format!("compositor: {message}"));
            }
            Ok(Some(_)) | Ok(None) => continue,
            Err(_) => break, // timeout or closed — request was delivered
        }
    }
    Ok(())
}

struct App {
    cfg: Cfg,
    dirty: bool,
    status: String,
    ipc_ok: Option<bool>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut app = App {
        cfg: load(),
        dirty: false,
        status: String::new(),
        ipc_ok: None,
    };
    if let Err(e) = cosmos_uitk::run("Settings", "cosmos.settings", (520, 480), move |ui| {
        draw(ui, &mut app)
    }) {
        tracing::error!("cosmos-settings fatal: {e}");
        std::process::exit(1);
    }
}

fn apply(app: &mut App) {
    match save(&app.cfg) {
        Ok(()) => app.status = "saved".into(),
        Err(e) => app.status = format!("save failed: {e}"),
    }
    app.dirty = false;

    // Push each key live; collect any errors into the status line.
    let pairs: Vec<(&str, serde_json::Value)> = vec![
        ("appearance", app.cfg.appearance.clone().into()),
        ("reduce_motion", app.cfg.reduce_motion.into()),
        ("scale", app.cfg.scale.into()),
        ("terminal", app.cfg.terminal.clone().into()),
        ("launcher_rows", app.cfg.launcher_rows.into()),
    ];
    let mut errs = Vec::new();
    for (k, v) in pairs {
        if let Err(e) = push(k, v) {
            errs.push(e);
        }
    }
    app.ipc_ok = Some(errs.is_empty());
    if !errs.is_empty() {
        app.status = format!("saved; live apply: {}", errs[0]);
    } else {
        app.status = "saved and applied".into();
    }
}

fn draw(ui: &mut egui::Ui, app: &mut App) {
    egui::CentralPanel::default().show(ui, |ui| {
        ui.heading("Appearance");
        ui.horizontal(|ui| {
            let mut dark = app.cfg.appearance == "dark";
            if ui.radio_value(&mut dark, true, "Dark").changed()
                || ui.radio_value(&mut dark, false, "Light").changed()
            {
                app.cfg.appearance = if dark { "dark" } else { "light" }.into();
                app.dirty = true;
            }
            if ui.checkbox(&mut app.cfg.reduce_motion, "Reduce motion").changed() {
                app.dirty = true;
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Scale");
            if ui
                .add(egui::Slider::new(&mut app.cfg.scale, 0.75..=2.0).step_by(0.05))
                .changed()
            {
                app.dirty = true;
            }
        });

        ui.add_space(10.0);
        ui.heading("Apps");
        ui.horizontal(|ui| {
            ui.label("Terminal");
            if ui
                .add(egui::TextEdit::singleline(&mut app.cfg.terminal).desired_width(160.0))
                .changed()
            {
                app.dirty = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Launcher rows");
            if ui
                .add(egui::DragValue::new(&mut app.cfg.launcher_rows).range(4..=20))
                .changed()
            {
                app.dirty = true;
            }
        });

        ui.add_space(10.0);
        ui.heading("Session");
        ui.horizontal(|ui| {
            ui.label("Compositor:");
            match app.ipc_ok {
                Some(true) => ui.label("connected"),
                Some(false) => ui.label("saved to disk (not connected)"),
                None => ui.label("—"),
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Restart session").clicked() {
                app.status = match push_quit() {
                    Ok(()) => "session ending…".into(),
                    Err(e) => e,
                };
            }
        });
        ui.label("compositor exits; the session supervisor restarts it");

        ui.add_space(14.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() {
                apply(app);
            }
            if !app.status.is_empty() {
                ui.label(&app.status);
            }
            if app.dirty {
                ui.label("(unsaved)");
            }
        });
    });
}

fn push_quit() -> Result<(), String> {
    let path = cosmos_ipc::socket_path();
    let mut stream =
        UnixStream::connect(&path).map_err(|e| format!("ipc connect {}: {e}", path.display()))?;
    cosmos_ipc::write_message(&mut stream, &Request::QuitSession).map_err(|e| e.to_string())
}

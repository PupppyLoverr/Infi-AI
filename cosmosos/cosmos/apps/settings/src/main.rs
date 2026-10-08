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
    appearance: String, // "dark" | "light"
    reduce_motion: bool,
    scale: f64,
    terminal: String,
    launcher_rows: u32,
    accent: String,        // accent preset name (cosmos_ipc::ACCENT_PRESETS)
    dock_position: String, // "left" | "right" | "bottom"
    wallpaper: String,     // wallpaper stem under /usr/share/cosmos/wallpapers
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            appearance: "dark".into(),
            reduce_motion: false,
            scale: 1.0,
            terminal: "cosmos-terminal".into(),
            launcher_rows: 10,
            accent: cosmos_ipc::DEFAULT_ACCENT.into(),
            dock_position: "left".into(),
            wallpaper: "violet".into(),
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
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"');
        match k.trim() {
            "appearance" => c.appearance = v.to_string(),
            "reduce_motion" => c.reduce_motion = v == "true",
            "scale" => c.scale = v.parse().unwrap_or(1.0),
            "terminal" => c.terminal = v.to_string(),
            "launcher_rows" => c.launcher_rows = v.parse().unwrap_or(10),
            "accent" => c.accent = v.to_string(),
            "dock_position" => c.dock_position = v.to_string(),
            "wallpaper" => c.wallpaper = v.to_string(),
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
        "# CosmosOS configuration\nappearance = \"{}\"\nreduce_motion = {}\nscale = {}\nterminal = \"{}\"\nlauncher_rows = {}\naccent = \"{}\"\ndock_position = \"{}\"\nwallpaper = \"{}\"\n",
        c.appearance, c.reduce_motion, c.scale, c.terminal, c.launcher_rows, c.accent, c.dock_position, c.wallpaper
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
        ("accent", app.cfg.accent.clone().into()),
        ("dock_position", app.cfg.dock_position.clone().into()),
        ("wallpaper", app.cfg.wallpaper.clone().into()),
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

const WALLPAPERS: &[&str] = &["violet", "ocean", "coral", "aurora", "peach", "indigo"];

thread_local! {
    static THUMBS: std::cell::RefCell<std::collections::HashMap<String, Option<egui::TextureHandle>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// 256×144 picker thumbnail of a shipped wallpaper — decoded once.
fn thumb(ctx: &egui::Context, stem: &str) -> Option<egui::TextureHandle> {
    THUMBS.with(|t| {
        t.borrow_mut()
            .entry(stem.to_string())
            .or_insert_with(|| {
                let dir = std::env::var("COSMOS_WALLPAPER_DIR")
                    .unwrap_or_else(|_| "/usr/share/cosmos/wallpapers".to_string());
                let img = image::open(std::path::Path::new(&dir).join(format!("{stem}-1920x1080.png")))
                    .ok()?
                    .to_rgba8();
                let small = image::imageops::resize(&img, 256, 144, image::imageops::FilterType::Triangle);
                let ci = egui::ColorImage::from_rgba_unmultiplied([256, 144], small.as_raw());
                Some(ctx.load_texture(format!("wp-{stem}"), ci, Default::default()))
            })
            .clone()
    })
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
            if ui
                .checkbox(&mut app.cfg.reduce_motion, "Reduce motion")
                .changed()
            {
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

        ui.add_space(6.0);
        // Accent presets — the Omarchy-style theme dial: one colour
        // reserved for active state, reskinned live over IPC.
        ui.horizontal(|ui| {
            ui.label("Accent");
            let dark = app.cfg.appearance == "dark";
            for (name, label, d_rgb, l_rgb) in cosmos_ipc::ACCENT_PRESETS {
                let [r, g, b] = if dark { *d_rgb } else { *l_rgb };
                let swatch = egui::Color32::from_rgb(r, g, b);
                let selected = app.cfg.accent == *name;
                let resp = ui
                    .add(
                        egui::Button::new(egui::RichText::new(*label).small())
                            .fill(if selected {
                                swatch
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if selected {
                                    swatch
                                } else {
                                    egui::Color32::from_gray(90)
                                },
                            ))
                            .corner_radius(egui::CornerRadius::same(10)),
                    )
                    .on_hover_text(*label);
                if resp.clicked() {
                    app.cfg.accent = (*name).into();
                    app.dirty = true;
                }
            }
        });

        ui.add_space(6.0);
        ui.label("Wallpaper");
        ui.horizontal_wrapped(|ui| {
            let dark = app.cfg.appearance == "dark";
            let ring = ui.visuals().selection.stroke.color;
            let hover = ui.visuals().widgets.hovered.bg_fill;
            for name in WALLPAPERS {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(128.0, 72.0), egui::Sense::click());
                if let Some(t) = thumb(ui.ctx(), &cosmos_ipc::wallpaper_for(name, dark)) {
                    egui::Image::from_texture(&t)
                        .corner_radius(8)
                        .paint_at(ui, rect);
                }
                let selected = app.cfg.wallpaper == *name;
                if selected || resp.hovered() {
                    ui.painter().rect_stroke(
                        rect.expand(2.0),
                        egui::CornerRadius::same(10),
                        egui::Stroke::new(2.0, if selected { ring } else { hover }),
                        egui::StrokeKind::Outside,
                    );
                }
                if resp.on_hover_text(*name).clicked() {
                    app.cfg.wallpaper = (*name).into();
                    app.dirty = true;
                }
            }
        });

        ui.add_space(6.0);
        // Dock edge — the rail re-anchors live via the compositor's
        // exclusive-zone layout.
        ui.horizontal(|ui| {
            ui.label("Dock position");
            for (v, label) in [("left", "Left"), ("right", "Right"), ("bottom", "Bottom")] {
                if ui
                    .radio_value(&mut app.cfg.dock_position, v.into(), label)
                    .changed()
                {
                    app.dirty = true;
                }
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

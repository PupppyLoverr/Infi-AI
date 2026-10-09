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
use cosmos_kit::controls::{button, segmented, slider, text_field, toggle, ButtonKind};
use cosmos_kit::layout::{group, sidebar_item, toolbar_title, AppWindow};
use cosmos_kit::{Icon, Kit};
use cosmos_theme::space;

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
    status: String,
    pane: Pane,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Appearance,
    Wallpaper,
    Dock,
    Apps,
    Session,
}

impl Pane {
    const ALL: [Pane; 5] = [
        Pane::Appearance,
        Pane::Wallpaper,
        Pane::Dock,
        Pane::Apps,
        Pane::Session,
    ];
    fn title(self) -> &'static str {
        match self {
            Pane::Appearance => "Appearance",
            Pane::Wallpaper => "Wallpaper",
            Pane::Dock => "Desktop & Dock",
            Pane::Apps => "Apps",
            Pane::Session => "Session",
        }
    }
    fn icon(self) -> Icon {
        match self {
            Pane::Appearance => Icon::Appearance,
            Pane::Wallpaper => Icon::Image,
            Pane::Dock => Icon::Desktop,
            Pane::Apps => Icon::Grid,
            Pane::Session => Icon::Info,
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut app = App {
        cfg: load(),
        status: String::new(),
        pane: Pane::Appearance,
    };
    if let Err(e) = cosmos_uitk::run("Settings", "cosmos.settings", (760, 520), move |ui| {
        draw(ui, &mut app)
    }) {
        tracing::error!("cosmos-settings fatal: {e}");
        std::process::exit(1);
    }
}

/// Save, then push just `key` off the UI thread. `push` waits out its
/// 500ms read timeout (SetConfig has no success ack), so pushing inline
/// would stall the window on every change.
fn set(app: &mut App, key: &'static str, value: serde_json::Value) {
    if let Err(e) = save(&app.cfg) {
        app.status = format!("save failed: {e}");
        return;
    }
    std::thread::spawn(move || {
        if let Err(e) = push(key, value) {
            tracing::warn!("{key} push: {e}");
        }
    });
    app.status.clear();
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
                let img =
                    image::open(std::path::Path::new(&dir).join(format!("{stem}-1920x1080.png")))
                        .ok()?
                        .to_rgba8();
                let small =
                    image::imageops::resize(&img, 256, 144, image::imageops::FilterType::Triangle);
                let ci = egui::ColorImage::from_rgba_unmultiplied([256, 144], small.as_raw());
                Some(ctx.load_texture(format!("wp-{stem}"), ci, Default::default()))
            })
            .clone()
    })
}

fn draw(ui: &mut egui::Ui, app: &mut App) {
    let kit = Kit::get(ui.ctx());
    let pane = app.pane;
    let cell = std::cell::RefCell::new(&mut *app);
    AppWindow::new()
        .toolbar(|ui| toolbar_title(ui, pane.title()))
        .sidebar(|ui| {
            let mut app = cell.borrow_mut();
            ui.add_space(4.0);
            for p in Pane::ALL {
                if sidebar_item(ui, p.icon(), p.title(), app.pane == p).clicked() {
                    app.pane = p;
                }
            }
        })
        .show(ui, |ui| {
            let mut app = cell.borrow_mut();
            let app = &mut **app;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(24, 20))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width().min(620.0));
                            match pane {
                                Pane::Appearance => appearance(ui, &kit, app),
                                Pane::Wallpaper => wallpaper(ui, &kit, app),
                                Pane::Dock => dock(ui, app),
                                Pane::Apps => apps(ui, app),
                                Pane::Session => session(ui, &kit, app),
                            }
                            if !app.status.is_empty() {
                                ui.add_space(space::S12);
                                ui.label(egui::RichText::new(&app.status).color(kit.text2()));
                            }
                        });
                });
        });
}

fn appearance(ui: &mut egui::Ui, kit: &Kit, app: &mut App) {
    group(ui, None, |g| {
        g.row("Appearance", |ui| {
            let mut i = usize::from(app.cfg.appearance == "dark");
            if segmented(ui, &mut i, &["Light", "Dark"]).changed() {
                app.cfg.appearance = ["light", "dark"][i].into();
                set(app, "appearance", app.cfg.appearance.clone().into());
            }
        });
        g.row("Accent colour", |ui| accent_swatches(ui, kit, app));
        g.row_detail(
            "Reduce motion",
            Some("Cross-fade instead of zoom and slide"),
            |ui| {
                if toggle(ui, &mut app.cfg.reduce_motion).changed() {
                    set(app, "reduce_motion", app.cfg.reduce_motion.into());
                }
            },
        );
    });
    ui.add_space(space::S20);
    group(ui, Some("Display"), |g| {
        g.row("Scale", |ui| {
            ui.label(
                egui::RichText::new(format!("{:.0}%", app.cfg.scale * 100.0)).color(kit.text2()),
            );
            ui.add_space(space::S8);
            let mut v = app.cfg.scale as f32;
            let r = slider(ui, &mut v, 0.75..=2.0, 200.0);
            if r.changed() {
                app.cfg.scale = ((v as f64) * 20.0).round() / 20.0;
            }
            if r.drag_stopped() || (r.clicked() && r.changed()) {
                set(app, "scale", app.cfg.scale.into());
            }
        });
    });
}

/// Preset swatches right-to-left (the row lays out from the right edge);
/// the selected one gets an outer ring.
fn accent_swatches(ui: &mut egui::Ui, kit: &Kit, app: &mut App) {
    let dark = app.cfg.appearance == "dark";
    for (name, label, d_rgb, l_rgb) in cosmos_ipc::ACCENT_PRESETS.iter().rev() {
        let [r, g, b] = if dark { *d_rgb } else { *l_rgb };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
        let c = rect.center();
        ui.painter()
            .circle_filled(c, 8.0, egui::Color32::from_rgb(r, g, b));
        if app.cfg.accent == *name {
            ui.painter()
                .circle_stroke(c, 10.5, egui::Stroke::new(1.5, kit.text2()));
        } else if resp.hovered() {
            ui.painter()
                .circle_stroke(c, 10.5, egui::Stroke::new(1.0, kit.hairline()));
        }
        if resp.on_hover_text(*label).clicked() {
            app.cfg.accent = (*name).into();
            set(app, "accent", app.cfg.accent.clone().into());
        }
    }
}

fn wallpaper(ui: &mut egui::Ui, kit: &Kit, app: &mut App) {
    let dark = app.cfg.appearance == "dark";
    group(ui, None, |g| {
        g.custom(|ui| {
            if let Some(t) = thumb(
                ui.ctx(),
                &cosmos_ipc::wallpaper_for(&app.cfg.wallpaper, dark),
            ) {
                let w = ui.available_width().min(320.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(w, w * 9.0 / 16.0), egui::Sense::hover());
                egui::Image::from_texture(&t)
                    .corner_radius(10)
                    .paint_at(ui, rect);
            }
            ui.add_space(space::S8);
            ui.label(
                egui::RichText::new(title_case(&app.cfg.wallpaper))
                    .strong()
                    .color(kit.text()),
            );
            ui.label(
                egui::RichText::new(
                    "The accent colour follows the wallpaper when set to Wallpaper.",
                )
                .size(12.0)
                .color(kit.text2()),
            );
        });
    });
    ui.add_space(space::S20);
    group(ui, Some("Dynamic Wallpapers"), |g| {
        g.custom(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::S12, space::S12);
            ui.horizontal_wrapped(|ui| {
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
                            rect.expand(3.0),
                            egui::CornerRadius::same(11),
                            egui::Stroke::new(
                                if selected { 2.5 } else { 1.0 },
                                if selected {
                                    kit.accent()
                                } else {
                                    kit.hairline()
                                },
                            ),
                            egui::StrokeKind::Outside,
                        );
                    }
                    // A picker click applies at once, like macOS.
                    if resp.on_hover_text(title_case(name)).clicked() {
                        app.cfg.wallpaper = (*name).into();
                        set(app, "wallpaper", app.cfg.wallpaper.clone().into());
                        app.status = "wallpaper applied".into();
                    }
                }
            });
        });
    });
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn dock(ui: &mut egui::Ui, app: &mut App) {
    group(ui, Some("Dock"), |g| {
        g.row("Position on screen", |ui| {
            const POS: [&str; 3] = ["left", "bottom", "right"];
            let mut i = POS
                .iter()
                .position(|p| *p == app.cfg.dock_position)
                .unwrap_or(0);
            if segmented(ui, &mut i, &["Left", "Bottom", "Right"]).changed() {
                app.cfg.dock_position = POS[i].into();
                set(app, "dock_position", app.cfg.dock_position.clone().into());
            }
        });
    });
    ui.add_space(space::S20);
    group(ui, Some("Search"), |g| {
        g.row_detail(
            "Results shown",
            Some("Rows in the Search-or-Ask launcher"),
            |ui| {
                ui.label(app.cfg.launcher_rows.to_string());
                ui.add_space(space::S8);
                let mut v = app.cfg.launcher_rows as f32;
                let r = slider(ui, &mut v, 4.0..=20.0, 160.0);
                if r.changed() {
                    app.cfg.launcher_rows = v.round() as u32;
                }
                if r.drag_stopped() || (r.clicked() && r.changed()) {
                    set(app, "launcher_rows", app.cfg.launcher_rows.into());
                }
            },
        );
    });
}

fn apps(ui: &mut egui::Ui, app: &mut App) {
    group(ui, Some("Default apps"), |g| {
        g.row_detail("Terminal", Some("Command run by super+Enter"), |ui| {
            let r = text_field(ui, &mut app.cfg.terminal, "cosmos-terminal", 200.0, false);
            if r.lost_focus() && !app.cfg.terminal.trim().is_empty() {
                set(app, "terminal", app.cfg.terminal.trim().to_string().into());
            }
        });
    });
}

fn session(ui: &mut egui::Ui, kit: &Kit, app: &mut App) {
    let connected = UnixStream::connect(cosmos_ipc::socket_path()).is_ok();
    group(ui, None, |g| {
        g.row("Compositor", |ui| {
            let (t, c) = if connected {
                ("Connected", kit.text2())
            } else {
                ("Not running — changes are saved to disk", kit.text3())
            };
            ui.label(egui::RichText::new(t).color(c));
        });
        g.row("Settings file", |ui| {
            ui.label(egui::RichText::new(config_path().display().to_string()).color(kit.text2()));
        });
    });
    ui.add_space(space::S20);
    group(ui, None, |g| {
        g.row_detail(
            "Restart session",
            Some("The compositor exits and the session supervisor restarts it"),
            |ui| {
                if button(ui, ButtonKind::Secondary, "Restart…").clicked() {
                    app.status = match push_quit() {
                        Ok(()) => "session ending…".into(),
                        Err(e) => e,
                    };
                }
            },
        );
    });
}

fn push_quit() -> Result<(), String> {
    let path = cosmos_ipc::socket_path();
    let mut stream =
        UnixStream::connect(&path).map_err(|e| format!("ipc connect {}: {e}", path.display()))?;
    cosmos_ipc::write_message(&mut stream, &Request::QuitSession).map_err(|e| e.to_string())
}

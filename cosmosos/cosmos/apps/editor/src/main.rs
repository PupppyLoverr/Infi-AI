//! cosmos-editor — a small real text editor for CosmosOS.
//! Open/save real files, Ctrl+S, dirty tracking, word/line counts.

use std::fs;
use std::path::PathBuf;

struct Editor {
    path: Option<PathBuf>,
    text: String,
    dirty: bool,
    status: String,
    open_field: String,
    show_open: bool,
}

impl Editor {
    fn new() -> Self {
        let mut e = Editor {
            path: None,
            text: String::new(),
            dirty: false,
            status: String::new(),
            open_field: String::new(),
            show_open: false,
        };
        // `cosmos-editor <file>` opens that file directly.
        if let Some(arg) = std::env::args().nth(1) {
            e.load(PathBuf::from(arg));
        }
        e
    }

    fn load(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(s) => {
                self.text = s;
                self.path = Some(path.clone());
                self.dirty = false;
                self.status = format!("loaded {}", path.display());
            }
            Err(e) => {
                // A missing file becomes a new buffer at that path — standard
                // editor behaviour, not a fake success.
                if e.kind() == std::io::ErrorKind::NotFound {
                    self.text.clear();
                    self.path = Some(path.clone());
                    self.dirty = false;
                    self.status = format!("new file {}", path.display());
                } else {
                    self.status = format!("load failed: {e}");
                }
            }
        }
    }

    fn save(&mut self) {
        let Some(path) = self.path.clone() else {
            self.show_open = true;
            self.status = "choose a path to save".into();
            return;
        };
        match fs::write(&path, &self.text) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("saved {}", path.display());
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut ed = Editor::new();
    if let Err(e) = cosmos_uitk::run("Editor", "cosmos.editor", (560, 420), move |ui| {
        draw(ui, &mut ed)
    }) {
        tracing::error!("cosmos-editor fatal: {e}");
        std::process::exit(1);
    }
}

fn draw(ui: &mut egui::Ui, ed: &mut Editor) {
    // Ctrl+S saves from anywhere in the window.
    if ui
        .ctx()
        .input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command)
    {
        ed.save();
    }

    egui::Panel::top("tools").show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Open").clicked() {
                ed.show_open = true;
            }
            if ui.button("Save").clicked() {
                ed.save();
            }
            ui.separator();
            let title = ed
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "untitled".into());
            ui.label(if ed.dirty {
                format!("{title} *")
            } else {
                title
            });
        });
    });

    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            let lines = ed.text.lines().count();
            let words = ed.text.split_whitespace().count();
            ui.label(format!("{lines} lines · {words} words"));
            ui.separator();
            if !ed.status.is_empty() {
                ui.label(&ed.status);
            }
        });
    });

    egui::CentralPanel::default().show(ui, |ui| {
        let before = ed.text.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut ed.text)
                    .font(egui::FontId::monospace(14.0))
                    .desired_width(f32::MAX)
                    .desired_rows(40)
                    .lock_focus(true),
            );
        });
        if ed.text != before {
            ed.dirty = true;
        }
    });

    if ed.show_open {
        let mut keep = true;
        egui::Window::new("Open / Save as")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label("Path:");
                ui.add(
                    egui::TextEdit::singleline(&mut ed.open_field)
                        .desired_width(300.0)
                        .hint_text("/home/cosmos/file.txt"),
                );
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        let p = ed.open_field.trim();
                        if !p.is_empty() {
                            ed.load(PathBuf::from(p));
                            keep = false;
                        }
                    }
                    if ui.button("Save here").clicked() {
                        let p = ed.open_field.trim();
                        if !p.is_empty() {
                            ed.path = Some(PathBuf::from(p));
                            ed.save();
                            keep = false;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        if !keep {
            ed.show_open = false;
        }
    }
}

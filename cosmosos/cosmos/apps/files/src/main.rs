//! cosmos-files — the CosmosOS file manager.
//! Real filesystem operations: browse, open, create folders, rename, delete,
//! copy paths. No fake entries — everything comes from std::fs.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    modified: String,
}

struct Files {
    dir: PathBuf,
    entries: Vec<Entry>,
    status: String,
    new_folder: Option<String>,
    rename: Option<(PathBuf, String)>,
    confirm_delete: Option<PathBuf>,
    path_edit: Option<String>,
}

impl Files {
    fn new() -> Self {
        let dir = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"));
        let mut f = Files {
            dir,
            entries: Vec::new(),
            status: String::new(),
            new_folder: None,
            rename: None,
            confirm_delete: None,
            path_edit: None,
        };
        f.refresh();
        f
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.status.clear();
        match fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let md = e.metadata().ok();
                    let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or_else(|| {
                        e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    });
                    let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
                    let modified = md
                        .and_then(|m| m.modified().ok())
                        .map(fmt_time)
                        .unwrap_or_default();
                    self.entries.push(Entry {
                        name: e.file_name().to_string_lossy().into_owned(),
                        path: e.path(),
                        is_dir,
                        size,
                        modified,
                    });
                }
                self.entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            Err(e) => self.status = format!("cannot read {}: {e}", self.dir.display()),
        }
    }

    fn open(&mut self, e: &Entry) {
        if e.is_dir {
            self.dir = e.path.clone();
            self.refresh();
        } else {
            // Open with the file's registered handler (xdg-open resolves to a
            // Cosmos app or whatever the image provides).
            match std::process::Command::new("xdg-open").arg(&e.path).spawn() {
                Ok(_) => self.status = format!("opened {}", e.name),
                Err(err) => self.status = format!("open failed: {err}"),
            }
        }
    }
}

fn fmt_time(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // civil-from-days (Howard Hinnant)
    let z = (secs / 86400) as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn fmt_size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut files = Files::new();
    if let Err(e) = cosmos_uitk::run("Files", "cosmos.files", (560, 400), move |ui| {
        draw(ui, &mut files)
    }) {
        tracing::error!("cosmos-files fatal: {e}");
        std::process::exit(1);
    }
}

fn draw(ui: &mut egui::Ui, f: &mut Files) {
    // Path-bar text field contents kept in state for editing.
    egui::Panel::top("bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Up").clicked() {
                if let Some(p) = f.dir.parent().map(|p| p.to_path_buf()) {
                    f.dir = p;
                    f.refresh();
                }
            }
            if ui.button("Home").clicked() {
                if let Ok(h) = std::env::var("HOME") {
                    f.dir = PathBuf::from(h);
                    f.refresh();
                }
            }
            ui.separator();
            let path_str = f.path_edit.get_or_insert_with(|| f.dir.display().to_string());
            let resp = ui.add(
                egui::TextEdit::singleline(path_str)
                    .desired_width(ui.available_width() - 140.0),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let p = PathBuf::from(path_str.trim());
                if p.is_dir() {
                    f.dir = p;
                } else {
                    f.status = format!("not a directory: {}", p.display());
                }
                f.path_edit = None;
                f.refresh();
            }
            if ui.button("Refresh").clicked() {
                f.refresh();
            }
            if ui.button("New folder").clicked() {
                f.new_folder = Some("untitled".into());
            }
        });
    });

    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!("{} items", f.entries.len()));
            ui.separator();
            if !f.status.is_empty() {
                ui.label(&f.status);
            }
        });
    });

    egui::Panel::left("places")
        .resizable(false)
        .exact_size(120.0)
        .show(ui, |ui| {
            ui.heading("Places");
            ui.separator();
            for (label, path) in [
                ("Home", std::env::var("HOME").unwrap_or_else(|_| "/".into())),
                ("Root", "/".into()),
                ("Etc", "/etc".into()),
                ("Tmp", "/tmp".into()),
                ("Var", "/var".into()),
            ] {
                if ui.selectable_label(false, label).clicked() {
                    f.dir = PathBuf::from(path);
                    f.refresh();
                }
            }
        });

    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("list")
                .num_columns(4)
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Type");
                    ui.strong("Size");
                    ui.strong("Modified");
                    ui.end_row();
                    let mut open_target: Option<Entry> = None;
                    let mut delete_target: Option<PathBuf> = None;
                    let mut rename_target: Option<(PathBuf, String)> = None;
                    let mut copy_path: Option<String> = None;
                    for e in &f.entries {
                        let name = if e.is_dir {
                            format!("{} /", e.name)
                        } else {
                            e.name.clone()
                        };
                        let resp = ui.selectable_label(false, name);
                        if resp.clicked() {
                            open_target = Some(Entry {
                                name: e.name.clone(),
                                path: e.path.clone(),
                                is_dir: e.is_dir,
                                size: e.size,
                                modified: e.modified.clone(),
                            });
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Copy path").clicked() {
                                copy_path = Some(e.path.display().to_string());
                                ui.close();
                            }
                            if ui.button("Rename").clicked() {
                                rename_target = Some((e.path.clone(), e.name.clone()));
                                ui.close();
                            }
                            if ui.button("Delete").clicked() {
                                delete_target = Some(e.path.clone());
                                ui.close();
                            }
                        });
                        ui.label(if e.is_dir { "folder" } else { "file" });
                        ui.label(if e.is_dir {
                            String::new()
                        } else {
                            fmt_size(e.size)
                        });
                        ui.label(&e.modified);
                        ui.end_row();
                    }
                    if let Some(e) = open_target {
                        f.open(&e);
                    }
                    if let Some(p) = delete_target {
                        f.confirm_delete = Some(p);
                    }
                    if let Some((p, n)) = rename_target {
                        f.rename = Some((p, n));
                    }
                    if let Some(p) = copy_path {
                        // Wayland clipboard comes later; the status bar shows
                        // the full path so nothing is silently dropped.
                        f.status = format!("path: {p}");
                    }
                });
        });
    });

    // New-folder modal
    if let Some(name) = f.new_folder.clone() {
        let mut keep = true;
        egui::Window::new("New folder")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                let mut name = name;
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0));
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() && !name.trim().is_empty() {
                        let p = f.dir.join(name.trim());
                        match fs::create_dir(&p) {
                            Ok(()) => {
                                f.status = format!("created {}", p.display());
                                f.refresh();
                            }
                            Err(e) => f.status = format!("create failed: {e}"),
                        }
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
                f.new_folder = Some(name);
            });
        if !keep {
            f.new_folder = None;
        }
    }

    // Rename modal
    if let Some((path, name)) = f.rename.clone() {
        let mut keep = true;
        egui::Window::new("Rename")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                let mut name = name;
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0));
                ui.horizontal(|ui| {
                    if ui.button("Rename").clicked() && !name.trim().is_empty() {
                        let to = path.with_file_name(name.trim());
                        match fs::rename(&path, &to) {
                            Ok(()) => {
                                f.status = format!("renamed to {}", to.display());
                                f.refresh();
                            }
                            Err(e) => f.status = format!("rename failed: {e}"),
                        }
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
                f.rename = Some((path.clone(), name));
            });
        if !keep {
            f.rename = None;
        }
    }

    // Delete confirmation
    if let Some(path) = f.confirm_delete.clone() {
        let mut keep = true;
        egui::Window::new("Delete")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(format!("Delete {}?", path.display()));
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        let r = if path.is_dir() {
                            fs::remove_dir_all(&path)
                        } else {
                            fs::remove_file(&path)
                        };
                        match r {
                            Ok(()) => {
                                f.status = format!("deleted {}", path.display());
                                f.refresh();
                            }
                            Err(e) => f.status = format!("delete failed: {e}"),
                        }
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        if !keep {
            f.confirm_delete = None;
        }
    }
}

//! cosmos-files — the CosmosOS file manager.
//! Real filesystem operations: browse, open, create folders, rename, delete,
//! copy paths. No fake entries — everything comes from std::fs.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

mod preview;

use cosmos_kit::ask;
use cosmos_kit::controls::{button, search_field, segmented_with, text_field, ButtonKind};
use cosmos_kit::layout::{
    grid_tile, menu_item, menu_section, menu_separator, sheet, sidebar_item, sidebar_section,
    status_text, submenu, toolbar_button, toolbar_spacer, toolbar_title, AppWindow, ColAlign,
    Column, Table,
};
use cosmos_kit::{icons, Icon, Kit};
use cosmos_theme::{radius, space, text};
use cosmos_uitk::icons::FileKind;

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
    back: Vec<PathBuf>,
    fwd: Vec<PathBuf>,
    selected: Option<PathBuf>,
    /// 0 = list, 1 = grid.
    view: usize,
    /// Toolbar search: filters the current folder by name.
    query: String,
    /// Portal file-chooser mode: rows select instead of opening, and a
    /// bottom bar offers Cancel/Choose (or Save). Selected paths are
    /// printed to stdout on confirm — cosmos-portal reads them.
    chooser: Option<Chooser>,
    /// Dotfiles are hidden unless toggled with Ctrl+H.
    show_hidden: bool,
    /// Last `(dir, rows drawn, footer count)` written to the log — the
    /// drive harness asserts rows == footer from these lines.
    logged_listing: Option<(PathBuf, usize, usize)>,
    /// Ask Cosmos result sheet (context menu → Ask Cosmos).
    ask: Option<ask::Job>,
    ask_pending: Option<(ask::Action, PathBuf)>,
    /// Finder-style preview column (Cmd+Shift+P / toolbar).
    preview_pane: bool,
    /// Space: Quick Look sheet for the selection.
    quick_look: bool,
    preview: Option<preview::Preview>,
}

struct Chooser {
    /// "open" | "save"
    mode: String,
    multiple: bool,
    directory: bool,
    selected: std::collections::BTreeSet<PathBuf>,
    save_name: String,
    /// Glob suffixes like `*.png` from the caller's filter list —
    /// empty means "show everything".
    filters: Vec<String>,
}

impl Chooser {
    fn matches(&self, e: &Entry) -> bool {
        if e.is_dir {
            return true;
        }
        self.filters.is_empty() || self.filters.iter().any(|g| glob_match(g, &e.name))
    }
}

/// `*.ext` / `*` glob matching — enough for portal filter patterns.
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" || pattern == name {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    false
}

/// Finish the chooser: print selected paths (one per line) and exit.
/// Exit code 0 = chosen, 3 = cancelled — the portal maps those to
/// Response codes 0 and 1.
fn chooser_finish(paths: &[PathBuf]) -> ! {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for p in paths {
        let _ = writeln!(out, "{}", p.display());
    }
    let _ = out.flush();
    std::process::exit(0);
}

fn chooser_cancel() -> ! {
    std::process::exit(3);
}

impl Files {
    fn new() -> Self {
        let dir = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));
        let mut f = Files {
            dir,
            entries: Vec::new(),
            status: String::new(),
            new_folder: None,
            rename: None,
            confirm_delete: None,
            path_edit: None,
            back: Vec::new(),
            fwd: Vec::new(),
            selected: None,
            view: 0,
            query: String::new(),
            chooser: None,
            show_hidden: false,
            logged_listing: None,
            ask: None,
            ask_pending: None,
            preview_pane: false,
            quick_look: false,
            preview: None,
        };
        f.refresh();
        f
    }

    /// Whether `e` gets a row — the footer count and the list share it.
    fn shows(&self, e: &Entry) -> bool {
        (self.show_hidden || !e.name.starts_with('.'))
            && (self.query.is_empty() || e.name.to_lowercase().contains(&self.query.to_lowercase()))
            && self.chooser.as_ref().map(|c| c.matches(e)).unwrap_or(true)
    }

    fn shown_count(&self) -> usize {
        self.entries.iter().filter(|e| self.shows(e)).count()
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.status.clear();
        match fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let md = e.metadata().ok();
                    let is_dir = md
                        .as_ref()
                        .map(|m| m.is_dir())
                        .unwrap_or_else(|| e.file_type().map(|t| t.is_dir()).unwrap_or(false));
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

    fn nav(&mut self, dir: PathBuf) {
        if dir != self.dir {
            self.back.push(std::mem::replace(&mut self.dir, dir));
            self.fwd.clear();
        }
        self.selected = None;
        self.refresh();
    }

    fn go_back(&mut self) {
        if let Some(d) = self.back.pop() {
            self.fwd.push(std::mem::replace(&mut self.dir, d));
            self.selected = None;
            self.refresh();
        }
    }

    fn go_forward(&mut self) {
        if let Some(d) = self.fwd.pop() {
            self.back.push(std::mem::replace(&mut self.dir, d));
            self.selected = None;
            self.refresh();
        }
    }

    /// Freedesktop trash: move into ~/.local/share/Trash/files and write
    /// the matching .trashinfo so the item can be restored.
    fn trash(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        let root = PathBuf::from(home).join(".local/share/Trash");
        fs::create_dir_all(root.join("files"))?;
        fs::create_dir_all(root.join("info"))?;
        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "item".into());
        let mut name = base.clone();
        let mut n = 2;
        while root.join("files").join(&name).exists()
            || root.join("info").join(format!("{name}.trashinfo")).exists()
        {
            name = format!("{base} {n}");
            n += 1;
        }
        let info = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            path.display(),
            fmt_datetime(SystemTime::now())
        );
        fs::write(root.join("info").join(format!("{name}.trashinfo")), info)?;
        if let Err(e) = fs::rename(path, root.join("files").join(&name)) {
            let _ = fs::remove_file(root.join("info").join(format!("{name}.trashinfo")));
            return Err(e);
        }
        Ok(())
    }

    fn open(&mut self, e: &Entry) {
        if e.is_dir {
            self.nav(e.path.clone());
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

/// `YYYY-MM-DDThh:mm:ss` (UTC) for .trashinfo.
fn fmt_datetime(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day = secs % 86400;
    format!(
        "{}T{:02}:{:02}:{:02}",
        fmt_time(t),
        day / 3600,
        day / 60 % 60,
        day % 60
    )
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
    // Portal chooser mode: `cosmos-files --chooser` with options in
    // COSMOS_CHOOSER_OPTS ("mode=open|save;multiple=1;directory=1;
    // name=…;filters=*.png,*.jpg") and optional COSMOS_CHOOSER_FOLDER.
    let mut title = "Files";
    let args: Vec<String> = std::env::args().collect();
    // `cosmos-files <folder>` opens there (the desktop's New Folder).
    if let Some(p) = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
    {
        if p.is_dir() {
            files.dir = p;
            files.refresh();
        }
    }
    if args.iter().any(|a| a == "--chooser") {
        let opts = std::env::var("COSMOS_CHOOSER_OPTS").unwrap_or_default();
        let get = |k: &str| {
            opts.split(';')
                .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                .map(str::to_string)
                .unwrap_or_default()
        };
        let mode = get("mode");
        files.chooser = Some(Chooser {
            mode: mode.clone(),
            multiple: get("multiple") == "1",
            directory: get("directory") == "1",
            selected: Default::default(),
            save_name: get("name"),
            filters: get("filters")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        });
        if let Ok(folder) = std::env::var("COSMOS_CHOOSER_FOLDER") {
            let p = PathBuf::from(folder);
            if p.is_dir() {
                files.dir = p;
                files.refresh();
            }
        }
        title = if mode == "save" {
            "Save File"
        } else {
            "Open File"
        };
    }
    if let Err(e) = cosmos_uitk::run(title, "cosmos.files", (640, 420), move |ui| {
        draw(ui, &mut files)
    }) {
        tracing::error!("cosmos-files fatal: {e}");
        std::process::exit(1);
    }
}

/// Sidebar places that exist on this machine.
type Places = Vec<(Icon, &'static str, PathBuf)>;

fn places() -> (Places, Places) {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let mut fav = vec![(Icon::Home, "Home", home.clone())];
    for (icon, label, sub) in [
        (Icon::Desktop, "Desktop", "Desktop"),
        (Icon::Document, "Documents", "Documents"),
        (Icon::Download, "Downloads", "Downloads"),
        (Icon::Image, "Pictures", "Pictures"),
        (Icon::Music, "Music", "Music"),
        (Icon::Sparkle, "Agents", "Agents"),
    ] {
        let p = home.join(sub);
        if p.is_dir() {
            fav.push((icon, label, p));
        }
    }
    let mut loc = vec![
        (Icon::Disk, "CosmosOS", PathBuf::from("/")),
        (Icon::Folder, "Temporary", PathBuf::from("/tmp")),
    ];
    let trash = home.join(".local/share/Trash/files");
    if trash.is_dir() {
        loc.push((Icon::Trash, "Trash", trash));
    }
    (fav, loc)
}

#[derive(Default)]
struct RowEvents {
    select: Option<PathBuf>,
    toggle: Option<PathBuf>,
    open: Option<usize>,
    rename: Option<(PathBuf, String)>,
    trash: Option<PathBuf>,
    copy: Option<String>,
    ask: Option<(ask::Action, PathBuf)>,
    agent: Option<PathBuf>,
    onboard: bool,
}

fn entry_menu(ui: &mut egui::Ui, e: &Entry, idx: usize, ev: &mut RowEvents) {
    if menu_item(ui, Some(Icon::FolderOpen), "Open", Some("Enter")).clicked() {
        ev.open = Some(idx);
        ui.close();
    }
    if menu_item(ui, Some(Icon::Pencil), "Rename", Some("F2")).clicked() {
        ev.rename = Some((e.path.clone(), e.name.clone()));
        ui.close();
    }
    if menu_item(ui, Some(Icon::Clipboard), "Copy Path", None).clicked() {
        ev.copy = Some(e.path.display().to_string());
        ui.close();
    }
    menu_separator(ui);
    menu_section(ui, "Ask Cosmos");
    if ask::provider_configured() {
        submenu(ui, Some(Icon::Sparkle), "Ask Cosmos", |ui| {
            for a in [ask::Action::Summarize, ask::Action::Explain] {
                if menu_item(ui, None, a.label(), None).clicked() {
                    ev.ask = Some((a, e.path.clone()));
                    ui.close();
                }
            }
            menu_separator(ui);
            if menu_item(ui, Some(Icon::Terminal), "Open in Agent", None).clicked() {
                ev.agent = Some(e.path.clone());
                ui.close();
            }
        });
    } else if menu_item(ui, Some(Icon::Sparkle), "Set up an agent…", None).clicked() {
        ev.onboard = true;
        ui.close();
    }
    menu_separator(ui);
    if menu_item(ui, Some(Icon::Trash), "Move to Trash", Some("Del")).clicked() {
        ev.trash = Some(e.path.clone());
        ui.close();
    }
}

fn row_click(f: &Files, e: &Entry, idx: usize, resp: &egui::Response, ev: &mut RowEvents) {
    if resp.double_clicked() {
        ev.open = Some(idx);
    } else if resp.clicked() {
        if f.chooser.is_some() && !e.is_dir {
            ev.toggle = Some(e.path.clone());
        } else {
            ev.select = Some(e.path.clone());
        }
    }
}

fn apply(f: &mut Files, ev: RowEvents) {
    if let Some(p) = ev.toggle {
        if let Some(c) = &mut f.chooser {
            if !c.directory {
                if c.multiple {
                    if !c.selected.remove(&p) {
                        c.selected.insert(p);
                    }
                } else {
                    c.selected.clear();
                    c.selected.insert(p);
                }
            }
        }
    }
    if let Some(p) = ev.select {
        f.selected = Some(p);
    }
    if let Some(p) = ev.trash {
        move_to_trash(f, p);
    }
    if let Some(r) = ev.rename {
        f.rename = Some(r);
    }
    if let Some(p) = ev.copy {
        f.status = format!("path: {p}");
    }
    if let Some((a, p)) = ev.ask {
        f.ask_pending = Some((a, p));
    }
    if let Some(p) = ev.agent {
        if let Err(e) = ask::open_in_agent(&p) {
            f.status = format!("can't open the agent: {e}");
        }
    }
    if ev.onboard {
        if let Err(e) = ask::open_onboarding() {
            f.status = format!("can't open Agents: {e}");
        }
    }
    if let Some(i) = ev.open {
        let e = &f.entries[i];
        let e = Entry {
            name: e.name.clone(),
            path: e.path.clone(),
            is_dir: e.is_dir,
            size: e.size,
            modified: e.modified.clone(),
        };
        f.open(&e);
    }
}

/// Trash, or offer permanent deletion when the item can't be moved
/// (another filesystem, or already inside the Trash).
fn move_to_trash(f: &mut Files, p: PathBuf) {
    let in_trash = p.to_string_lossy().contains("/.local/share/Trash/");
    if in_trash {
        f.confirm_delete = Some(p);
        return;
    }
    match f.trash(&p) {
        Ok(()) => {
            f.status = format!(
                "moved {} to Trash",
                p.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default()
            );
            f.selected = None;
            f.refresh();
        }
        Err(e) => {
            f.status = format!("can't move to Trash: {e}");
            f.confirm_delete = Some(p);
        }
    }
}

fn keyboard(ui: &egui::Ui, f: &mut Files) {
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let modal = f.new_folder.is_some()
        || f.rename.is_some()
        || f.confirm_delete.is_some()
        || f.path_edit.is_some();
    if modal {
        return;
    }
    // Match shortcuts against each key event's own modifiers: a quick
    // Ctrl+L can release Ctrl before the frame that sees the L press.
    use egui::Modifiers as M;
    let key = |k| ui.input(|i| i.key_pressed(k));
    let chord = |m, k| ui.ctx().input_mut(|i| i.consume_key(m, k));
    if chord(M::COMMAND | M::SHIFT, egui::Key::N) {
        f.new_folder = Some("untitled folder".into());
    }
    if chord(M::COMMAND | M::SHIFT, egui::Key::P) {
        f.preview_pane = !f.preview_pane;
    }
    if chord(M::COMMAND, egui::Key::H) {
        f.show_hidden = !f.show_hidden;
    }
    if chord(M::COMMAND, egui::Key::L) {
        f.path_edit = Some(f.dir.display().to_string());
    }
    if chord(M::ALT, egui::Key::ArrowLeft) {
        f.go_back();
    }
    if chord(M::ALT, egui::Key::ArrowRight) {
        f.go_forward();
    }
    if chord(M::ALT, egui::Key::ArrowUp) {
        if let Some(p) = f.dir.parent().map(|p| p.to_path_buf()) {
            f.nav(p);
        }
    }
    if typing {
        return;
    }
    let sel = f
        .selected
        .as_ref()
        .and_then(|p| f.entries.iter().position(|e| &e.path == p));
    if let Some(i) = sel {
        if key(egui::Key::Enter) {
            apply(
                f,
                RowEvents {
                    open: Some(i),
                    ..Default::default()
                },
            );
        } else if key(egui::Key::Delete) {
            let p = f.entries[i].path.clone();
            move_to_trash(f, p);
        } else if key(egui::Key::Space) {
            f.quick_look = !f.quick_look;
        } else if key(egui::Key::F2) {
            f.rename = Some((f.entries[i].path.clone(), f.entries[i].name.clone()));
        }
    }
}

/// Keep `f.preview` loading whatever the pane or Quick Look shows.
fn sync_preview(ctx: &egui::Context, f: &mut Files) {
    if f.selected.is_none() {
        f.quick_look = false;
    }
    let want = f
        .selected
        .clone()
        .filter(|_| f.preview_pane || f.quick_look);
    match (want, &f.preview) {
        (Some(p), Some(pv)) if pv.path == p => {}
        (Some(p), _) => f.preview = Some(preview::Preview::start(ctx, p)),
        (None, _) => f.preview = None,
    }
    if let Some(pv) = f.preview.as_mut() {
        pv.poll(ctx);
    }
}

fn caption(ui: &mut egui::Ui, kit: &Kit, s: &str) {
    ui.label(
        egui::RichText::new(s)
            .size(text::CAPTION.size)
            .color(kit.text2()),
    );
}

/// Paint `content` for `e` into `r`: fitted image, text head, or icon.
fn paint_content(
    ui: &egui::Ui,
    kit: &Kit,
    e: &Entry,
    content: Option<&preview::Content>,
    r: egui::Rect,
    mono: f32,
) {
    use preview::Content;
    match content {
        Some(Content::Image(tex, _)) => {
            let ir = preview::fit(r, tex.size_vec2());
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().add(
                egui::epaint::RectShape::filled(ir, radius::ROW, egui::Color32::WHITE)
                    .with_texture(tex.id(), uv),
            );
        }
        Some(Content::Text(t)) if !t.trim().is_empty() => {
            ui.painter().rect_stroke(
                r,
                radius::ROW,
                egui::Stroke::new(1.0, kit.hairline()),
                egui::StrokeKind::Inside,
            );
            let inner = r.shrink(space::S8);
            let lines = ((inner.height() / (mono * 1.35)).floor() as usize).max(1);
            let head: String = t.lines().take(lines).collect::<Vec<_>>().join("\n");
            let g = ui.painter().layout(
                head,
                egui::FontId::monospace(mono),
                kit.text2(),
                inner.width(),
            );
            ui.painter()
                .with_clip_rect(inner)
                .galley(inner.min, g, kit.text2());
        }
        _ => {
            let kind = FileKind::of(&e.name, e.is_dir);
            let side = r.height().min(r.width()).min(96.0);
            cosmos_uitk::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(r.center(), egui::vec2(side, side)),
                kind,
            );
        }
    }
}

/// One-line facts under the preview: kind, size or item count, pixels.
fn facts(e: &Entry, content: Option<&preview::Content>) -> String {
    use preview::Content;
    let kind = FileKind::of(&e.name, e.is_dir).label().to_string();
    match content {
        Some(Content::Folder(n)) => format!("{kind} · {n} item{}", if *n == 1 { "" } else { "s" }),
        Some(Content::Image(_, [w, h])) => format!("{kind} · {} · {w}×{h}", fmt_size(e.size)),
        _ if e.is_dir => kind,
        _ => format!("{kind} · {}", fmt_size(e.size)),
    }
}

const PANE_W: f32 = 260.0;
/// The list keeps at least this width before the pane is dropped.
const PANE_MIN_LIST: f32 = 380.0;

fn preview_pane(ui: &mut egui::Ui, kit: &Kit, e: Option<&Entry>, pv: Option<&preview::Preview>) {
    let Some(e) = e else {
        let r = ui.max_rect();
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "No Selection",
            egui::FontId::proportional(text::BODY.size),
            kit.text3(),
        );
        return;
    };
    let content = pv.map(|p| &p.content);
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, w * 0.75), egui::Sense::hover());
    paint_content(ui, kit, e, content, r, 10.5);
    ui.add_space(space::S12);
    ui.label(
        egui::RichText::new(&e.name)
            .size(text::BODY.size)
            .strong()
            .color(kit.text()),
    );
    caption(ui, kit, &facts(e, content));
    caption(ui, kit, &format!("Modified {}", e.modified));
}

fn quick_look(ctx: &egui::Context, kit: &Kit, f: &mut Files) {
    let Some(e) = f
        .selected
        .as_ref()
        .and_then(|p| f.entries.iter().find(|e| &e.path == p))
    else {
        f.quick_look = false;
        return;
    };
    let content = f.preview.as_ref().map(|p| &p.content);
    let screen = ctx.content_rect();
    let w = (screen.width() * 0.7).clamp(320.0, 900.0);
    let h = (screen.height() * 0.7).clamp(240.0, 640.0);
    let mut close = false;
    let resp = egui::Modal::new(egui::Id::new("files-quick-look"))
        .frame(
            cosmos_kit::layout::menu_frame(kit)
                .inner_margin(space::S16)
                .corner_radius(radius::PANEL),
        )
        .backdrop_color(egui::Color32::from_black_alpha(90))
        .show(ctx, |ui| {
            ui.set_width(w);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&e.name)
                        .size(text::TITLE3.size)
                        .strong()
                        .color(kit.text()),
                );
                caption(ui, kit, &facts(e, content));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= toolbar_button(ui, Icon::Close, "Close", false).clicked();
                });
            });
            ui.add_space(space::S8);
            match content {
                Some(preview::Content::Text(t)) if !t.trim().is_empty() => {
                    egui::ScrollArea::vertical()
                        .max_height(h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(t.as_str())
                                    .font(egui::FontId::monospace(12.0))
                                    .color(kit.text()),
                            );
                        });
                }
                _ => {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                    paint_content(ui, kit, e, content, r, 12.0);
                }
            }
        });
    if close || resp.should_close() {
        f.quick_look = false;
    }
}

fn draw(ui: &mut egui::Ui, f: &mut Files) {
    let kit = Kit::get(ui.ctx());
    keyboard(ui, f);
    sync_preview(ui.ctx(), f);
    let (fav, loc) = places();
    let title = f
        .dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "CosmosOS".into());
    let cell = std::cell::RefCell::new(&mut *f);
    AppWindow::new()
        .toolbar(|ui| {
            let mut f = cell.borrow_mut();
            let f = &mut **f;
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_enabled_ui(!f.back.is_empty(), |ui| {
                if toolbar_button(ui, Icon::Back, "Back", false).clicked() {
                    f.go_back();
                }
            });
            ui.add_enabled_ui(!f.fwd.is_empty(), |ui| {
                if toolbar_button(ui, Icon::Forward, "Forward", false).clicked() {
                    f.go_forward();
                }
            });
            toolbar_title(ui, &title);
            let fixed = 2.0 * 32.0 + 2.0 * 28.0 + 4.0 + 3.0 * space::S12;
            let search_w = (ui.available_width() - fixed - space::S8).clamp(96.0, 200.0);
            toolbar_spacer(ui, fixed + search_w);
            segmented_with(ui, &mut f.view, 2, 32.0, |ui, i, r, c| {
                let icon = [Icon::List, Icon::Grid][i];
                icons::paint(
                    ui,
                    icon,
                    egui::Rect::from_center_size(r.center(), egui::vec2(16.0, 16.0)),
                    c,
                );
            });
            ui.add_space(space::S12);
            search_field(ui, &mut f.query, "Search", search_w);
            ui.add_space(space::S12);
            if toolbar_button(ui, Icon::FolderPlus, "New Folder", false).clicked() {
                f.new_folder = Some("untitled folder".into());
            }
            if toolbar_button(ui, Icon::Sidebar, "Preview (Ctrl+Shift+P)", f.preview_pane).clicked()
            {
                f.preview_pane = !f.preview_pane;
            }
        })
        .sidebar(|ui| {
            let mut f = cell.borrow_mut();
            for (head, list) in [("Favourites", &fav), ("Locations", &loc)] {
                sidebar_section(ui, head);
                for (icon, label, path) in list {
                    if sidebar_item(ui, *icon, label, &f.dir == path).clicked() {
                        f.nav(path.clone());
                    }
                }
            }
        })
        .status(|ui| {
            let mut f = cell.borrow_mut();
            let f = &mut **f;
            let dir = f.dir.clone();
            let status = f.status.clone();
            if let Some(c) = f.chooser.as_mut() {
                if c.mode == "save" {
                    status_text(ui, "Save as:");
                    text_field(ui, &mut c.save_name, "Name", 180.0, false);
                }
                status_text(ui, &status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if c.mode == "save" { "Save" } else { "Open" };
                    if button(ui, ButtonKind::Primary, label).clicked() {
                        let paths: Vec<PathBuf> = if c.mode == "save" {
                            vec![dir.join(c.save_name.trim())]
                        } else if c.directory {
                            vec![dir.clone()]
                        } else {
                            c.selected.iter().cloned().collect()
                        };
                        chooser_finish(&paths);
                    }
                    if button(ui, ButtonKind::Plain, "Cancel").clicked() {
                        chooser_cancel();
                    }
                });
            } else {
                let shown = f.shown_count();
                let hidden = f
                    .entries
                    .iter()
                    .filter(|e| !f.show_hidden && e.name.starts_with('.'))
                    .count();
                let mut text = match shown {
                    1 => "1 item".to_string(),
                    n => format!("{n} items"),
                };
                if hidden > 0 {
                    text.push_str(&format!(", {hidden} hidden"));
                }
                status_text(ui, &text);
                if !f.status.is_empty() {
                    status_text(ui, "·");
                    status_text(ui, &f.status);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_text(ui, &f.dir.display().to_string());
                });
            }
        })
        .show(ui, |ui| {
            let mut f = cell.borrow_mut();
            let f = &mut **f;
            let mut rows = 0usize;
            let mut ev = RowEvents::default();
            if f.preview_pane && ui.available_width() >= PANE_W + PANE_MIN_LIST {
                let sel = f
                    .selected
                    .as_ref()
                    .and_then(|p| f.entries.iter().find(|e| &e.path == p));
                egui::Panel::right("files-preview")
                    .exact_size(PANE_W)
                    .resizable(false)
                    .frame(egui::Frame::NONE.inner_margin(space::S12))
                    .show(ui, |ui| preview_pane(ui, &kit, sel, f.preview.as_ref()));
            }
            let picked = |f: &Files, e: &Entry| {
                f.chooser
                    .as_ref()
                    .map(|c| c.selected.contains(&e.path))
                    .unwrap_or(false)
                    || f.selected.as_ref() == Some(&e.path)
            };
            if f.view == 0 {
                let cols = [
                    Column {
                        title: "Name",
                        width: 0.0,
                        align: ColAlign::Left,
                    },
                    Column {
                        title: "Size",
                        width: 84.0,
                        align: ColAlign::Right,
                    },
                    Column {
                        title: "Kind",
                        width: 124.0,
                        align: ColAlign::Left,
                    },
                    Column {
                        title: "Date Modified",
                        width: 112.0,
                        align: ColAlign::Left,
                    },
                ];
                let table = Table { cols: &cols };
                ui.add_space(4.0);
                table.header(ui);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(4.0);
                        for (i, e) in f.entries.iter().enumerate() {
                            if !f.shows(e) {
                                continue;
                            }
                            rows += 1;
                            let kind = FileKind::of(&e.name, e.is_dir);
                            let size = if e.is_dir {
                                "—".to_string()
                            } else {
                                fmt_size(e.size)
                            };
                            let lead = move |ui: &egui::Ui, r: egui::Rect| {
                                cosmos_uitk::icons::paint(ui.painter(), r.expand(1.0), kind)
                            };
                            let resp = table.row(
                                ui,
                                picked(f, e),
                                Some(&lead),
                                &[&e.name, &size, kind.label(), &e.modified],
                            );
                            row_click(f, e, i, &resp, &mut ev);
                            resp.context_menu(|ui| entry_menu(ui, e, i, &mut ev));
                        }
                    });
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::new().inner_margin(space::S12).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing = egui::vec2(space::S8, space::S12);
                            ui.horizontal_wrapped(|ui| {
                                for (i, e) in f.entries.iter().enumerate() {
                                    if !f.shows(e) {
                                        continue;
                                    }
                                    rows += 1;
                                    let kind = FileKind::of(&e.name, e.is_dir);
                                    let resp = grid_tile(ui, picked(f, e), &e.name, |ui, r| {
                                        cosmos_uitk::icons::paint(ui.painter(), r.shrink(6.0), kind)
                                    });
                                    row_click(f, e, i, &resp, &mut ev);
                                    resp.context_menu(|ui| entry_menu(ui, e, i, &mut ev));
                                }
                            });
                        });
                    });
            }
            if rows == 0 {
                let r = ui.max_rect();
                let c = egui::pos2(r.center().x, r.top() + r.height() * 0.4);
                let only_hidden = !f.entries.is_empty() && !f.show_hidden && f.query.is_empty();
                icons::paint(
                    ui,
                    if f.query.is_empty() {
                        Icon::Folder
                    } else {
                        Icon::Search
                    },
                    egui::Rect::from_center_size(c, egui::vec2(40.0, 40.0)),
                    kit.text3(),
                );
                let msg = if !f.query.is_empty() {
                    format!("No items match “{}”", f.query)
                } else if only_hidden {
                    format!(
                        "Only hidden items ({}). Ctrl+H shows them.",
                        f.entries.len()
                    )
                } else {
                    "This folder is empty".to_string()
                };
                ui.painter().text(
                    c + egui::vec2(0.0, 36.0),
                    egui::Align2::CENTER_CENTER,
                    msg,
                    egui::FontId::proportional(13.0),
                    kit.text2(),
                );
            }
            let footer = f.shown_count();
            let listing = (f.dir.clone(), rows, footer);
            if f.logged_listing.as_ref() != Some(&listing) {
                tracing::info!(dir = %f.dir.display(), rows, footer, "files: listing");
                f.logged_listing = Some(listing);
            }
            apply(f, ev);
        });
    let ctx = ui.ctx().clone();
    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));

    if f.quick_look {
        quick_look(&ctx, &kit, f);
    }

    if let Some((a, p)) = f.ask_pending.take() {
        f.ask = Some(ask::Job::start(&ctx, a, ask::Subject::Path(p)));
    }
    if f.ask
        .as_ref()
        .is_some_and(|job| ask::result_sheet(&ctx, job, false) != ask::SheetResult::Open)
    {
        f.ask = None;
    }

    if let Some(mut name) = f.new_folder.take() {
        let mut done = false;
        let open = sheet(
            &ctx,
            egui::Id::new("files-new-folder"),
            "New Folder",
            |ui| {
                text_field(ui, &mut name, "Name", 360.0, false).request_focus();
                ui.add_space(space::S16);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if (button(ui, ButtonKind::Primary, "Create").clicked() || enter)
                        && !name.trim().is_empty()
                    {
                        let p = f.dir.join(name.trim());
                        match fs::create_dir(&p) {
                            Ok(()) => {
                                f.status = format!("created {}", name.trim());
                                f.refresh();
                                f.selected = Some(p);
                            }
                            Err(e) => f.status = format!("create failed: {e}"),
                        }
                        done = true;
                    }
                    if button(ui, ButtonKind::Secondary, "Cancel").clicked() {
                        done = true;
                    }
                });
            },
        );
        if open && !done {
            f.new_folder = Some(name);
        }
    }

    if let Some((path, mut name)) = f.rename.take() {
        let mut done = false;
        let open = sheet(&ctx, egui::Id::new("files-rename"), "Rename", |ui| {
            text_field(ui, &mut name, "Name", 360.0, false).request_focus();
            ui.add_space(space::S16);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if (button(ui, ButtonKind::Primary, "Rename").clicked() || enter)
                    && !name.trim().is_empty()
                {
                    let to = path.with_file_name(name.trim());
                    match fs::rename(&path, &to) {
                        Ok(()) => {
                            f.status = format!("renamed to {}", name.trim());
                            f.refresh();
                            f.selected = Some(to);
                        }
                        Err(e) => f.status = format!("rename failed: {e}"),
                    }
                    done = true;
                }
                if button(ui, ButtonKind::Secondary, "Cancel").clicked() {
                    done = true;
                }
            });
        });
        if open && !done {
            f.rename = Some((path, name));
        }
    }

    if let Some(path) = f.confirm_delete.take() {
        let mut done = false;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let open = sheet(
            &ctx,
            egui::Id::new("files-delete"),
            "Delete Permanently?",
            |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "“{name}” will be deleted immediately. This can't be undone."
                    ))
                    .color(kit.text2()),
                );
                ui.add_space(space::S16);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, ButtonKind::Destructive, "Delete").clicked() {
                        let r = if path.is_dir() {
                            fs::remove_dir_all(&path)
                        } else {
                            fs::remove_file(&path)
                        };
                        match r {
                            Ok(()) => {
                                f.status = format!("deleted {name}");
                                f.selected = None;
                                f.refresh();
                            }
                            Err(e) => f.status = format!("delete failed: {e}"),
                        }
                        done = true;
                    }
                    if button(ui, ButtonKind::Secondary, "Cancel").clicked() {
                        done = true;
                    }
                });
            },
        );
        if open && !done {
            f.confirm_delete = Some(path);
        }
    }

    if let Some(mut text) = f.path_edit.take() {
        let mut done = false;
        let open = sheet(&ctx, egui::Id::new("files-goto"), "Go to Folder", |ui| {
            text_field(ui, &mut text, "/path/to/folder", 360.0, false).request_focus();
            ui.add_space(space::S16);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, ButtonKind::Primary, "Go").clicked() || enter {
                    let p = PathBuf::from(text.trim());
                    if p.is_dir() {
                        f.nav(p);
                    } else {
                        f.status = format!("not a folder: {}", p.display());
                    }
                    done = true;
                }
                if button(ui, ButtonKind::Secondary, "Cancel").clicked() {
                    done = true;
                }
            });
        });
        if open && !done {
            f.path_edit = Some(text);
        }
    }
}

//! The launcher overlay: dim backdrop + a centred floating card —
//! Win11 Start's shape (search field, "Pinned" icon grid, app rows,
//! system-action footer) with macOS Spotlight's placement and Launchpad's
//! icon-grid idiom. Typing switches the card into a Spotlight results list.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{
    seat::keyboard::{KeyEvent, Keysym},
    shell::WaylandSurface,
};
use tiny_skia::{Color, PathBuilder, PixmapMut, Stroke};
use wayland_client::protocol::wl_shm;

use crate::{
    ask, desktop::AppEntry, dock, draw, glass, icons, preview, search, ShellState, LAUNCHER_WIDTH,
};

const INPUT_H: f64 = 48.0;
const ROW_H: f64 = 36.0;
const SEC_H: f64 = 24.0;
const FOOTER_H: f64 = 44.0;
const CARD_R: f32 = cosmos_theme::radius::PANEL;
const MAX_ROWS: usize = 5;
const GRID_COLS: usize = 6;
const CELL_H: f64 = 104.0;
const CELL_ICON: f32 = 56.0;
/// Gap between overlay top edge and the launcher box.
const START_GAP: f64 = 12.0;

/// Recently launched apps shown under the pinned grid, newest first.
const MAX_REC: usize = 3;
/// Widget card strip height in the non-searching (Start) layout.
const WIDGET_H: f64 = 60.0;
/// Footer avatar + name hit/draw width.
const USER_W: f64 = 220.0;
/// Search or Ask (super+Space): a 680×56 pill in the upper third whose
/// results expand beneath it in the same glass.
const SEARCH_W: f64 = 680.0;
const PILL_H: f64 = 56.0;
const SEARCH_ROWS: usize = 8;
/// Right-hand preview pane for a selected Files result.
const PREVIEW_W: f32 = 276.0;
const PREVIEW_MIN_H: f64 = 250.0;
const ASK_LINES: usize = 14;
const ASK_LINE_H: f32 = 19.0;

/// Keep the preview in step with the selected row (loaded once per path).
fn update_preview(state: &mut ShellState) {
    let rows = state.filtered_results();
    let path = rows.get(state.launcher_sel).and_then(|r| match &r.kind {
        search::Kind::File(p) => Some(p.clone()),
        _ => None,
    });
    match path {
        Some(p) if state.launcher_preview.as_ref().map(|v| v.path.as_str()) != Some(p.as_str()) => {
            state.launcher_preview = Some(preview::load(&p));
        }
        Some(_) => {}
        None => state.launcher_preview = None,
    }
}

/// The streamed answer, while the query is still the question asked.
fn ask_view(state: &ShellState) -> Option<(String, Option<Result<(), String>>)> {
    let q = state.launcher_query.strip_prefix('?')?.trim();
    (!state.ask_query.is_empty() && q == state.ask_query)
        .then(|| (state.ask_text.clone(), state.ask_done.clone()))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// Row item in the results/apps list.
    Item(usize),
    /// Icon cell in the pinned grid.
    Cell(usize),
    /// Row in the RECOMMENDED block (index into `recommended`).
    Recent(usize),
    /// Footer action: 0 = Settings, 1 = Log out.
    Action(u8),
    Input,
    List,
    Backdrop,
}

/// The pinned grid — the same set the dock pins, resolved to entries.
pub fn pinned(state: &ShellState) -> Vec<AppEntry> {
    dock::PINNED
        .iter()
        .filter_map(|id| state.apps.iter().find(|a| a.id == *id).cloned())
        .collect()
}

/// Win11's "Recommended": the last few apps the user actually launched,
/// resolved to entries (skipped when the desktop file vanished).
pub fn recommended(state: &ShellState) -> Vec<AppEntry> {
    state
        .recent
        .iter()
        .filter_map(|id| state.apps.iter().find(|a| &a.id == id).cloned())
        .take(MAX_REC)
        .collect()
}

/// Persisted MRU — real launch history, survives restarts.
fn recent_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })?;
    Some(base.join("cosmos-shell/recent.txt"))
}

/// Load the launch MRU (one desktop id per line, newest first).
pub fn load_recent() -> Vec<String> {
    let Some(path) = recent_file() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let id = line.split('\t').next().unwrap_or("").trim();
        if !id.is_empty() && !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    }
    out.truncate(8);
    out
}

/// Last-launch unix time per desktop id (`id\tsecs` lines; bare-id
/// lines from older shells carry no time).
pub fn recent_times() -> std::collections::HashMap<String, u64> {
    let text = recent_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let (id, ts) = l.split_once('\t')?;
            Some((id.trim().to_string(), ts.trim().parse().ok()?))
        })
        .collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "Just now" / "5 min ago" / "3 h ago" / "Yesterday" / "4 days ago".
fn ago(ts: u64) -> String {
    let d = now_secs().saturating_sub(ts);
    match d {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        86_400..=172_799 => "Yesterday".into(),
        _ => format!("{} days ago", d / 86_400),
    }
}

/// Write the launch MRU back to disk. `recent[0]` was just launched and
/// gets the current time; the rest keep their recorded times.
pub fn save_recent(recent: &[String]) {
    let Some(path) = recent_file() else {
        return;
    };
    let times = recent_times();
    let now = now_secs();
    let body: String = recent
        .iter()
        .enumerate()
        .map(|(i, id)| match (i, times.get(id)) {
            (0, _) => format!("{id}\t{now}\n"),
            (_, Some(t)) => format!("{id}\t{t}\n"),
            _ => format!("{id}\n"),
        })
        .collect();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, body);
}

/// Display name for the Start footer: passwd GECOS name, else $USER.
fn user_name() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let gecos = std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.split(':').next() == Some(user.as_str()))
                .and_then(|l| l.split(':').nth(4))
                .map(|g| g.split(',').next().unwrap_or("").trim().to_string())
        })
        .unwrap_or_default();
    if gecos.is_empty() {
        user
    } else {
        gecos
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// Start's card origin. With the dock at the bottom it sits just above
/// the dock, centred; with a side dock it hugs that edge under the
/// menubar. It never covers the menubar.
fn box_origin(size: (u32, u32), dock_pos: dock::DockPos, height: f64) -> (f64, f64) {
    let (w, h) = (size.0 as f64, size.1 as f64);
    let card_w = LAUNCHER_WIDTH as f64;
    let rail = dock::STRIP as f64 + START_GAP;
    let min_top = crate::PANEL_HEIGHT as f64 + START_GAP;
    let centred = ((w - card_w) / 2.0).max(8.0);
    let (left, top) = match dock_pos {
        dock::DockPos::Bottom => (centred, h - rail - height),
        dock::DockPos::Left => (rail.min((w - card_w).max(8.0)), min_top),
        dock::DockPos::Right => ((w - rail - card_w).max(8.0), min_top),
    };
    (left, top.max(min_top))
}

fn grid_rows(n_pinned: usize) -> usize {
    if n_pinned == 0 {
        0
    } else {
        (n_pinned + GRID_COLS - 1) / GRID_COLS
    }
}

/// Height of the RECOMMENDED block (header + rows), 0 when hidden.
fn rec_height(n_rec: usize, searching: bool) -> f64 {
    if searching || n_rec == 0 {
        0.0
    } else {
        SEC_H + n_rec as f64 * ROW_H
    }
}

/// Content height of the card.
fn box_height(n_items: usize, n_pinned: usize, n_rec: usize, searching: bool) -> f64 {
    let mut h = INPUT_H;
    if searching || n_pinned == 0 {
        h += SEC_H + n_items.min(MAX_ROWS) as f64 * ROW_H;
    } else {
        // Win11 Start: PINNED grid + RECOMMENDED + WIDGETS + footer.
        h += SEC_H + grid_rows(n_pinned) as f64 * CELL_H;
        h += rec_height(n_rec, searching);
        // WIDGETS header (already in widgets_top) + cards drawn at
        // +SEC_H+4 with height WIDGET_H-10, then a 16px gap above the
        // footer — the old sum came up 24px short and the footer cut
        // the cards (v3-04-start).
        h += 2.0 * SEC_H + WIDGET_H + 10.0;
    }
    h + FOOTER_H
}

/// Y offset (relative to card top) where the results list starts.
fn rows_top(n_pinned: usize, n_rec: usize, searching: bool) -> f64 {
    if searching || n_pinned == 0 {
        INPUT_H + SEC_H
    } else {
        INPUT_H + SEC_H + grid_rows(n_pinned) as f64 * CELL_H + rec_height(n_rec, searching) + SEC_H
    }
}

/// Y offset where the widget strip starts (non-searching layout).
fn widgets_top(n_pinned: usize, n_rec: usize) -> f64 {
    INPUT_H + SEC_H + grid_rows(n_pinned) as f64 * CELL_H + rec_height(n_rec, false) + SEC_H
}

// ---------------- Start widgets — all data read live ----------------

#[derive(Debug, Clone)]
pub struct Widget {
    pub icon: &'static str,
    pub big: String,
    pub small: String,
    /// 0..1 fill for the thin bar under the text (None = no bar).
    pub bar: Option<f32>,
}

/// Build the Start-panel widgets: the three system widgets plus any
/// agent-generated custom widgets under
/// `~/.local/share/cosmos/widgets/<name>/` (widget.toml + data.sh).
pub fn widgets(state: &ShellState) -> Vec<Widget> {
    let mut out = Vec::new();
    out.push(Widget {
        icon: "widget-clock",
        big: state.sysinfo.clock.clone(),
        small: state.sysinfo.date.clone(),
        bar: None,
    });

    // Memory + load: /proc/meminfo + /proc/loadavg.
    let (total_kb, avail_kb) = meminfo();
    let mem_used = if total_kb > 0 {
        (total_kb - avail_kb) as f32 / total_kb as f32
    } else {
        0.0
    };
    let free_gb = avail_kb as f64 / 1_048_576.0;
    let load = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|t| t.split_whitespace().next().map(str::to_string))
        .unwrap_or_else(|| "0".into());
    out.push(Widget {
        icon: "widget-cpu",
        big: format!("{:.0}%", mem_used * 100.0),
        small: format!("load {load} · {free_gb:.1}G free"),
        bar: Some(mem_used.clamp(0.0, 1.0)),
    });

    // Root filesystem fill: statvfs("/").
    if let Some((used_b, total_b)) = fs_usage("/") {
        let frac = if total_b > 0 {
            used_b as f32 / total_b as f32
        } else {
            0.0
        };
        let gb = 1_073_741_824.0;
        out.push(Widget {
            icon: "widget-disk",
            big: format!("{:.0}%", frac * 100.0),
            small: format!(
                "{:.1} of {:.1} GB used",
                used_b as f64 / gb,
                total_b as f64 / gb
            ),
            bar: Some(frac.clamp(0.0, 1.0)),
        });
    }
    out.extend(custom_widgets());
    out
}

// ---------------- Agent-generated custom widgets ----------------

/// `~/.local/share/cosmos/widgets/<name>/` → a widget defined by
/// `widget.toml` (`title`, `icon`, `refresh_secs`) plus `data.sh`,
/// which prints `big=`, `small=`, `bar=` (0-1) key=value lines. Runs
/// under /bin/sh as the user, once per refresh interval — results are
/// cached so per-frame draws never exec. A widget that fails or times
/// out (2s) is skipped, never blanked into the panel.
fn custom_widgets() -> Vec<Widget> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;

    struct Cached {
        widget: Widget,
        at: Instant,
        refresh: u64,
    }
    static CACHE: Mutex<Option<HashMap<std::path::PathBuf, Cached>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);

    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".local/share/cosmos/widgets");
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let wdir = entry.path();
        if !wdir.is_dir() {
            continue;
        }
        let meta = wdir.join("widget.toml");
        let script = wdir.join("data.sh");
        if !meta.is_file() || !script.is_file() {
            continue;
        }
        let (title, icon, refresh) = parse_widget_meta(&meta);
        let stale = cache
            .get(&wdir)
            .map(|c| c.at.elapsed().as_secs() >= c.refresh)
            .unwrap_or(true);
        if stale {
            if let Some(widget) = run_widget_script(&script, icon) {
                cache.insert(
                    wdir.clone(),
                    Cached {
                        widget,
                        at: Instant::now(),
                        refresh,
                    },
                );
            }
        }
        if let Some(c) = cache.get(&wdir) {
            let mut w = c.widget.clone();
            if w.small.is_empty() && !title.is_empty() {
                w.small = title.clone();
            }
            found.push(w);
        }
    }
    found
}

fn parse_widget_meta(path: &std::path::Path) -> (String, &'static str, u64) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut title = String::new();
    let mut icon = "widget-clock";
    let mut refresh = 30u64;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"');
        match k.trim() {
            "title" => title = v.to_string(),
            // icons.rs glyphs agents may pick from; unknown → generic.
            "icon" => {
                icon = match v {
                    "cpu" | "widget-cpu" => "widget-cpu",
                    "disk" | "widget-disk" => "widget-disk",
                    "clock" | "widget-clock" => "widget-clock",
                    _ => "widget-clock",
                }
            }
            "refresh_secs" | "refresh" => {
                refresh = v.parse().unwrap_or(30).clamp(5, 3600);
            }
            _ => {}
        }
    }
    (title, icon, refresh)
}

fn run_widget_script(script: &std::path::Path, icon: &'static str) -> Option<Widget> {
    // Agent-generated code runs here: 2s wall-clock cap, then SIGKILL —
    // a hung script must never stall a panel frame.
    let mut child = std::process::Command::new("/bin/sh")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break child.wait_with_output().ok(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(_) => break None,
        }
    }?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut w = Widget {
        icon,
        big: String::new(),
        small: String::new(),
        bar: None,
    };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "big" => w.big = v.trim().chars().take(24).collect(),
            "small" => w.small = v.trim().chars().take(48).collect(),
            "bar" => w.bar = v.trim().parse::<f32>().ok().map(|f| f.clamp(0.0, 1.0)),
            _ => {}
        }
    }
    (!w.big.is_empty() || !w.small.is_empty()).then_some(w)
}

fn meminfo() -> (u64, u64) {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return (0, 0);
    };
    let mut total = 0;
    let mut avail = 0;
    for line in text.lines() {
        let kb = |v: &str| {
            v.split_whitespace()
                .nth(1)
                .and_then(|n| n.parse::<u64>().ok())
        };
        if line.starts_with("MemTotal:") {
            total = kb(line).unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            avail = kb(line).unwrap_or(0);
        }
    }
    (total, avail)
}

/// (used bytes, total bytes) for the filesystem containing `path`.
fn fs_usage(path: &str) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(std::ffi::OsStr::new(path).as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let bs = st.f_frsize;
    Some(((st.f_blocks - st.f_bfree) * bs, st.f_blocks * bs))
}

/// Y offset where RECOMMENDED rows start (0 when hidden).
fn rec_top(n_pinned: usize, searching: bool) -> f64 {
    if searching {
        0.0
    } else {
        INPUT_H + SEC_H + grid_rows(n_pinned) as f64 * CELL_H
    }
}

/// Rows are only drawn while searching or when no pinned grid exists —
/// callers gate `hit_test` on this so invisible rows can't be clicked.
pub fn rows_shown(searching: bool, n_pinned: usize, n_items: usize) -> usize {
    if searching || n_pinned == 0 {
        n_items
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
pub fn hit_test(
    x: f64,
    y: f64,
    size: (u32, u32),
    n_items: usize,
    n_pinned: usize,
    n_rec: usize,
    searching: bool,
    dock_pos: dock::DockPos,
) -> Hit {
    let height = box_height(n_items, n_pinned, n_rec, searching);
    let (left, top) = box_origin(size, dock_pos, height);
    if x < left || x > left + LAUNCHER_WIDTH as f64 || y < top || y > top + height {
        return Hit::Backdrop;
    }
    let ry = y - top;
    if ry >= height - FOOTER_H {
        let btn_w = 104.0;
        let mid = LAUNCHER_WIDTH as f64;
        if x < left + 8.0 + USER_W {
            return Hit::Action(0);
        }
        if x > left + mid - 8.0 - btn_w {
            return Hit::Action(1);
        }
        return Hit::List;
    }
    if ry < INPUT_H {
        return Hit::Input;
    }
    if !searching && n_pinned > 0 {
        let grid_top = INPUT_H + SEC_H;
        let grid_h = grid_rows(n_pinned) as f64 * CELL_H;
        if ry >= grid_top && ry < grid_top + grid_h {
            let cell_w = LAUNCHER_WIDTH as f64 / GRID_COLS as f64;
            let gx = ((x - left) / cell_w) as usize;
            let gy = ((ry - grid_top) / CELL_H) as usize;
            let idx = gy * GRID_COLS + gx;
            if idx < n_pinned {
                return Hit::Cell(idx);
            }
            return Hit::List;
        }
    }
    if !searching && n_rec > 0 {
        let rtop = rec_top(n_pinned, searching) + SEC_H;
        let idx = ((ry - rtop) / ROW_H) as usize;
        if ry >= rtop && idx < n_rec {
            return Hit::Recent(idx);
        }
    }
    let rtop = rows_top(n_pinned, n_rec, searching);
    let idx = ((ry - rtop) / ROW_H) as usize;
    if ry >= rtop && idx < n_items.min(MAX_ROWS) {
        Hit::Item(idx)
    } else {
        Hit::List
    }
}

fn theme(dark: bool) -> (Color, Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2), // card
            draw::accent_soft(true),                   // selection wash
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF), // border
            Color::from_rgba8(0x24, 0x25, 0x29, 0xFF), // input field
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0xA8, 0xA8, 0xAE, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            draw::accent_soft(false),
            Color::from_rgba8(0xC4, 0xC4, 0xC8, 0xFF),
            Color::from_rgba8(0xEF, 0xEF, 0xF1, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x5A, 0x5A, 0x5E, 0xFF),
        )
    }
}

fn search_origin(size: (u32, u32)) -> (f64, f64) {
    (
        ((size.0 as f64 - SEARCH_W) / 2.0).max(8.0),
        (size.1 as f64 * 0.22).max(48.0),
    )
}

/// Y offsets (from the pill's top) of group headers and rows, and the
/// card's total height.
pub struct SearchLayout {
    pub headers: Vec<(f64, &'static str)>,
    pub rows: Vec<f64>,
    pub height: f64,
}

pub fn search_layout(rows: &[search::Row], query_empty: bool) -> SearchLayout {
    let mut out = SearchLayout {
        headers: Vec::new(),
        rows: Vec::new(),
        height: PILL_H,
    };
    if query_empty {
        return out;
    }
    let mut y = PILL_H + 6.0;
    if rows.is_empty() {
        out.height = y + ROW_H + 6.0;
        return out;
    }
    let mut prev = "";
    for (i, row) in rows.iter().take(SEARCH_ROWS).enumerate() {
        let label = if i == 0 {
            "TOP HIT"
        } else {
            search::group(&row.kind)
        };
        if label != prev {
            out.headers.push((y, label));
            y += SEC_H;
            prev = label;
        }
        out.rows.push(y);
        y += ROW_H;
    }
    out.height = y + 8.0;
    out
}

pub fn hit_test_search(
    x: f64,
    y: f64,
    size: (u32, u32),
    rows: &[search::Row],
    query_empty: bool,
    card_h: f64,
) -> Hit {
    let (left, top) = search_origin(size);
    let lay = search_layout(rows, query_empty);
    if x < left || x > left + SEARCH_W || y < top || y > top + lay.height.max(card_h) {
        return Hit::Backdrop;
    }
    let ry = y - top;
    if ry < PILL_H {
        return Hit::Input;
    }
    lay.rows
        .iter()
        .position(|&r| ry >= r && ry < r + ROW_H)
        .map(Hit::Item)
        .unwrap_or(Hit::List)
}

#[allow(clippy::too_many_arguments)]
fn draw_search(
    pixmap: &mut PixmapMut<'_>,
    size: (u32, u32),
    rows: &[search::Row],
    query: &str,
    sel: usize,
    dark: bool,
    preview: Option<&preview::FilePreview>,
    answer: Option<(String, Option<Result<(), String>>)>,
) -> f64 {
    let (box_bg, sel_bg, sep, _input_bg, fg, fg_dim) = theme(dark);
    let (left, top) = search_origin(size);
    let lay = search_layout(rows, query.is_empty());
    let sw = SEARCH_W as f32;
    let answer = answer.map(|(text, done)| {
        let mut lines = ask::wrap(&text, sw - 48.0, |s| draw::text_width(13.0, s));
        if lines.len() > ASK_LINES {
            lines.drain(..lines.len() - ASK_LINES);
        }
        (lines, done)
    });
    let height = match &answer {
        Some((lines, done)) => {
            let n = lines.len() + matches!(done, Some(Err(_))) as usize * 2;
            PILL_H + 46.0 + n.max(1) as f64 * ASK_LINE_H as f64 + 14.0
        }
        None if preview.is_some() => lay.height.max(PILL_H + 8.0 + PREVIEW_MIN_H + 8.0),
        None => lay.height,
    };
    let (l, t, ht) = (left as f32, top as f32, height as f32);
    let r = if query.is_empty() {
        PILL_H as f32 / 2.0
    } else {
        CARD_R
    };
    draw::shadow(pixmap, l, t, sw, ht, r);
    glass::fill_glass(
        pixmap,
        &crate::ShellState::wallpaper_name(),
        l,
        t,
        sw,
        ht,
        r,
        l,
        t,
        dark,
        box_bg,
    );
    draw::stroke_round_rect(
        pixmap,
        l + 0.5,
        t + 0.5,
        sw - 1.0,
        ht - 1.0,
        r - 0.5,
        1.0,
        sep,
    );

    // The pill: magnifier (or an "Ask" chip in ? mode), query, caret.
    let ask = query.starts_with('?');
    let shown = if ask { query[1..].trim_start() } else { query };
    let mid = t + PILL_H as f32 / 2.0;
    let text_x = if ask {
        draw::fill_round_rect(pixmap, l + 14.0, mid - 12.0, 46.0, 24.0, 12.0, sel_bg);
        draw::text_centered(pixmap, l + 14.0, mid - 8.0, 46.0, 16.0, 12.0, "Ask", fg);
        l + 70.0
    } else {
        magnifier(pixmap, l + 26.0, mid, fg_dim);
        l + 50.0
    };
    let (label, color) = match (shown.is_empty(), ask) {
        (false, _) => (shown, fg),
        (true, true) => ("Ask Cosmos anything", fg_dim),
        (true, false) => ("Search or Ask", fg_dim),
    };
    draw::text(
        pixmap,
        text_x,
        mid - 12.0,
        sw - (text_x - l) - 120.0,
        24.0,
        20.0,
        label,
        color,
    );
    let caret_x = text_x + shown.chars().count() as f32 * 10.4 + 1.0;
    draw::fill_rect(
        pixmap,
        caret_x,
        mid - 11.0,
        2.0,
        22.0,
        Color::from_rgba8(fg.r(), fg.g(), fg.b(), 0xFF),
    );
    if !ask {
        draw::text(
            pixmap,
            l + sw - 104.0,
            mid - 7.0,
            88.0,
            14.0,
            11.0,
            "Tab to Ask",
            fg_dim,
        );
    }
    if query.is_empty() {
        return height;
    }

    draw::fill_rect(pixmap, l + 12.0, t + PILL_H as f32, sw - 24.0, 1.0, sep);
    if let Some((lines, done)) = answer {
        draw_answer(
            pixmap,
            (l, t, sw, ht),
            &lines,
            done.as_ref(),
            dark,
            fg,
            fg_dim,
        );
        return height;
    }
    // Rows share the card with the preview pane when one is showing.
    let rw = if preview.is_some() {
        sw - PREVIEW_W - 20.0
    } else {
        sw
    };
    for &(y, label) in &lay.headers {
        section(
            pixmap,
            l + 16.0,
            t + y as f32,
            &label.to_uppercase(),
            fg_dim,
        );
    }
    if rows.is_empty() {
        draw::text(
            pixmap,
            l + 20.0,
            t + PILL_H as f32 + 14.0,
            sw - 40.0,
            18.0,
            13.0,
            "No results",
            fg_dim,
        );
    }
    let sel = sel.min(lay.rows.len().saturating_sub(1));
    for (i, (row, &y)) in rows.iter().zip(&lay.rows).enumerate() {
        let ry = t + y as f32;
        let rh = ROW_H as f32;
        if i == sel {
            draw::fill_round_rect(pixmap, l + 6.0, ry + 2.0, rw - 12.0, rh - 4.0, 8.0, sel_bg);
        }
        let big = i == 0;
        let isz = if big { 24.0 } else { 20.0 };
        icons::app_tile(pixmap, &row.icon, l + 16.0, ry + (rh - isz) / 2.0, isz);
        let title: String = row.title.chars().take(48).collect();
        let tx = l + 50.0;
        if big {
            draw::text_bold(
                pixmap,
                tx,
                ry + (rh - 18.0) / 2.0,
                rw * 0.5,
                18.0,
                14.0,
                &title,
                fg,
            );
        } else {
            draw::text(
                pixmap,
                tx,
                ry + (rh - 18.0) / 2.0,
                rw * 0.5,
                18.0,
                13.0,
                &title,
                fg,
            );
        }
        if !row.sub.is_empty() {
            let sx =
                tx + draw::text_width(if big { 14.0 } else { 13.0 }, &title).min(rw * 0.5) + 10.0;
            let sub: String = row.sub.chars().take(40).collect();
            draw::text(
                pixmap,
                sx,
                ry + (rh - 14.0) / 2.0,
                l + rw - 100.0 - sx,
                14.0,
                11.0,
                &sub,
                fg_dim,
            );
        }
        draw::text(
            pixmap,
            l + rw - 84.0,
            ry + (rh - 14.0) / 2.0,
            70.0,
            14.0,
            11.0,
            row.hint,
            fg_dim,
        );
    }
    if let Some(p) = preview {
        let (px, py) = (l + sw - PREVIEW_W - 12.0, t + PILL_H as f32 + 8.0);
        draw_preview(
            pixmap,
            p,
            px,
            py,
            ht - PILL_H as f32 - 16.0,
            sel_bg,
            fg,
            preview_dim(dark),
        );
    }
    height
}

#[allow(clippy::too_many_arguments)]
/// Secondary text on the preview pane: the pane sits on the accent wash,
/// so the card's `fg_dim` drops under 4.5:1 at 11px there.
fn preview_dim(dark: bool) -> CtColor {
    if dark {
        CtColor::rgba(0xC8, 0xC8, 0xCE, 0xFF)
    } else {
        CtColor::rgba(0x3A, 0x3A, 0x3E, 0xFF)
    }
}

fn draw_preview(
    pixmap: &mut PixmapMut<'_>,
    p: &preview::FilePreview,
    x: f32,
    y: f32,
    h: f32,
    bg: Color,
    fg: CtColor,
    fg_dim: CtColor,
) {
    draw::fill_round_rect(pixmap, x, y, PREVIEW_W, h, 12.0, bg);
    let inner = PREVIEW_W - 28.0;
    let body_h = preview::THUMB_H as f32;
    match &p.body {
        preview::Body::Image(img) => {
            let ix = x + (PREVIEW_W - img.width() as f32) / 2.0;
            let iy = y + 14.0 + (body_h - img.height() as f32) / 2.0;
            pixmap.draw_pixmap(
                ix.round() as i32,
                iy.round() as i32,
                img.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                draw::xf(),
                None,
            );
        }
        preview::Body::Text(lines) => {
            for (i, line) in lines.iter().enumerate() {
                draw::text_mono(
                    pixmap,
                    x + 14.0,
                    y + 14.0 + i as f32 * 16.0,
                    inner,
                    16.0,
                    11.0,
                    line,
                    fg_dim,
                );
            }
        }
        preview::Body::None => {
            let icon = if p.info.starts_with("Folder") {
                "cosmos-files"
            } else {
                "search-doc"
            };
            icons::app_tile(
                pixmap,
                icon,
                x + (PREVIEW_W - 64.0) / 2.0,
                y + 14.0 + (body_h - 64.0) / 2.0,
                64.0,
            );
        }
    }
    let ty = y + 14.0 + body_h + 14.0;
    let name: String = p.name.chars().take(34).collect();
    draw::text_bold(pixmap, x + 14.0, ty, inner, 18.0, 13.0, &name, fg);
    draw::text(
        pixmap,
        x + 14.0,
        ty + 20.0,
        inner,
        15.0,
        11.0,
        &p.info,
        fg_dim,
    );
    draw::text(
        pixmap,
        x + 14.0,
        ty + 36.0,
        inner,
        15.0,
        11.0,
        &p.modified,
        fg_dim,
    );
}

/// The streamed answer, edged with the accent gradient (spec §2.2).
#[allow(clippy::too_many_arguments)]
fn draw_answer(
    pixmap: &mut PixmapMut<'_>,
    (l, t, sw, ht): (f32, f32, f32, f32),
    lines: &[String],
    done: Option<&Result<(), String>>,
    dark: bool,
    fg: CtColor,
    fg_dim: CtColor,
) {
    let accent = draw::accent(dark);
    let fade =
        Color::from_rgba(accent.red(), accent.green(), accent.blue(), 0.15).unwrap_or(accent);
    for (width, alpha) in [(5.0, 0.22), (1.5, 1.0)] {
        let shader = tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(l, t),
            tiny_skia::Point::from_xy(l + sw, t + ht),
            vec![
                tiny_skia::GradientStop::new(0.0, accent),
                tiny_skia::GradientStop::new(0.5, fade),
                tiny_skia::GradientStop::new(1.0, accent),
            ],
            tiny_skia::SpreadMode::Pad,
            tiny_skia::Transform::identity(),
        );
        let (Some(path), Some(mut shader)) = (
            draw::round_rect_path(l + 0.75, t + 0.75, sw - 1.5, ht - 1.5, CARD_R - 0.75),
            shader,
        ) else {
            break;
        };
        shader.apply_opacity(alpha);
        let paint = tiny_skia::Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        let stroke = Stroke {
            width,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, draw::xf(), None);
    }
    let y0 = t + PILL_H as f32 + 14.0;
    let status = match done {
        None if lines.is_empty() => "Thinking…",
        None => "Answering…",
        Some(Ok(())) => "Done",
        Some(Err(_)) => "Couldn't answer",
    };
    draw::text_bold(pixmap, l + 24.0, y0, 200.0, 18.0, 13.0, "Cosmos", fg);
    draw::text(
        pixmap,
        l + sw - 144.0,
        y0 + 2.0,
        120.0,
        15.0,
        11.0,
        status,
        fg_dim,
    );
    let mut y = y0 + 28.0;
    for line in lines {
        draw::text(pixmap, l + 24.0, y, sw - 48.0, ASK_LINE_H, 13.0, line, fg);
        y += ASK_LINE_H;
    }
    if let Some(Err(e)) = done {
        let e: String = e.chars().take(90).collect();
        draw::text(pixmap, l + 24.0, y, sw - 48.0, ASK_LINE_H, 12.0, &e, fg_dim);
        draw::text(
            pixmap,
            l + 24.0,
            y + ASK_LINE_H,
            sw - 48.0,
            ASK_LINE_H,
            12.0,
            "Set up a model provider in Terminal: opencode auth login",
            fg_dim,
        );
    }
}

pub fn draw(state: &mut ShellState) {
    if state.launcher_search {
        update_preview(state);
    }
    let answer = if state.launcher_search {
        ask_view(state)
    } else {
        None
    };
    let (w, h) = state.launcher_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.launcher_surface.clone() else {
        return;
    };
    let rows = state.filtered_results();
    let pinned_apps = pinned(state);
    let rec_apps = recommended(state);
    let searching = !state.launcher_query.is_empty();
    let n_items = rows.len().min(MAX_ROWS);
    let hover = state.launcher_hover;
    let (box_bg, sel_bg, sep, input_bg, fg, fg_dim) = theme(state.dark);
    let glyph = Color::from_rgba8(fg.r(), fg.g(), fg.b(), fg.a());
    // Hoisted before the pool's mutable borrow below.
    let wigs = widgets(state);

    let (pw, ph) = draw::phys(w, h);
    let stride = pw as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("launcher: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    // The pool slot may still carry a previous surface's frame — the dim
    // scrim blends SrcOver, so stale bytes would show through as a dimmed
    // ghost. Clear to transparent first.
    pixmap.fill(Color::TRANSPARENT);
    if state.launcher_search {
        draw::scrim(&mut pixmap, w, h, state.dark);
        let card_h = draw_search(
            &mut pixmap,
            (w, h),
            &rows,
            &state.launcher_query,
            state.launcher_sel,
            state.dark,
            state.launcher_preview.as_ref(),
            answer,
        );
        state.launcher_card_h = card_h;
        let wl_surface = layer.wl_surface().clone();
        state.set_viewport(&wl_surface, w, h);
        buffer.attach_to(&wl_surface).ok();
        wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
        wl_surface.commit();
        return;
    }

    // Spotlight idiom: a vignette scrim — airy near the card, deeper at
    // the corners — then the card's own soft shadow.
    draw::scrim(&mut pixmap, w, h, state.dark);

    let height = box_height(rows.len(), pinned_apps.len(), rec_apps.len(), searching);
    let (left, top) = box_origin((w, h), state.dock_position, height);
    let (left, top, height) = (left as f32, top as f32, height as f32);
    draw::shadow(
        &mut pixmap,
        left,
        top,
        LAUNCHER_WIDTH as f32,
        height,
        CARD_R,
    );
    glass::fill_glass(
        &mut pixmap,
        &crate::ShellState::wallpaper_name(),
        left,
        top,
        LAUNCHER_WIDTH as f32,
        height,
        CARD_R,
        left,
        top,
        state.dark,
        box_bg,
    );
    draw::stroke_round_rect(
        &mut pixmap,
        left + 0.5,
        top + 0.5,
        LAUNCHER_WIDTH as f32 - 1.0,
        height - 1.0,
        CARD_R - 0.5,
        1.0,
        sep,
    );

    // Search field: inset rounded rect + magnifier glyph + query + caret.
    let field_x = left + 12.0;
    let field_y = top + 10.0;
    let field_w = LAUNCHER_WIDTH as f32 - 24.0;
    let field_h = INPUT_H as f32 - 20.0;
    draw::fill_round_rect(
        &mut pixmap,
        field_x,
        field_y,
        field_w,
        field_h,
        8.0,
        input_bg,
    );
    magnifier(&mut pixmap, field_x + 14.0, field_y + field_h / 2.0, fg_dim);

    let query_display = if state.launcher_query.is_empty() {
        "Search or Ask — > command, ? question"
    } else {
        state.launcher_query.as_str()
    };
    let query_color = if state.launcher_query.is_empty() {
        fg_dim
    } else {
        fg
    };
    draw::text(
        &mut pixmap,
        field_x + 36.0,
        field_y + (field_h - 18.0) / 2.0,
        field_w - 44.0,
        18.0,
        14.0,
        query_display,
        query_color,
    );
    let caret_x = field_x + 36.0 + state.launcher_query.len() as f32 * 7.8;
    draw::fill_rect(
        &mut pixmap,
        caret_x + 1.0,
        field_y + (field_h - 16.0) / 2.0,
        1.5,
        16.0,
        if state.dark {
            Color::from_rgba8(0xEC, 0xEC, 0xEE, 0xFF)
        } else {
            Color::from_rgba8(0x18, 0x18, 0x1B, 0xFF)
        },
    );

    if searching {
        // Spotlight idiom: "RESULTS" + rows.
        let sec_y = top + INPUT_H as f32;
        section(&mut pixmap, left + 16.0, sec_y, "RESULTS", fg_dim);
    } else if !pinned_apps.is_empty() {
        // Start idiom: "PINNED" + icon grid.
        let sec_y = top + INPUT_H as f32;
        section(&mut pixmap, left + 16.0, sec_y, "PINNED", fg_dim);
        let cell_w = LAUNCHER_WIDTH as f32 / GRID_COLS as f32;
        for (idx, app) in pinned_apps.iter().enumerate() {
            let gx = (idx % GRID_COLS) as f32;
            let gy = (idx / GRID_COLS) as f32;
            let cx = left + gx * cell_w;
            let cy = sec_y + SEC_H as f32 + gy * CELL_H as f32;
            // Start idiom: the hovered cell gets a quiet highlight.
            if hover == Some(Hit::Cell(idx)) {
                draw::fill_round_rect(
                    &mut pixmap,
                    cx + 3.0,
                    cy + 4.0,
                    cell_w - 6.0,
                    CELL_H as f32 - 8.0,
                    8.0,
                    sel_bg,
                );
            }
            icons::app_tile(
                &mut pixmap,
                &icons::key_for(&app.id),
                cx + (cell_w - CELL_ICON) / 2.0,
                cy + 12.0,
                CELL_ICON,
            );
            draw::text_centered(
                &mut pixmap,
                cx + 4.0,
                cy + 12.0 + CELL_ICON + 8.0,
                cell_w - 8.0,
                14.0,
                11.0,
                &app.name,
                fg,
            );
        }
    }

    // RECOMMENDED rows (Win11 Start) between the grid and ALL APPS.
    if !searching && !rec_apps.is_empty() {
        let rtop = rec_top(pinned_apps.len(), searching) as f32;
        section(&mut pixmap, left + 16.0, top + rtop, "RECOMMENDED", fg_dim);
        let times = recent_times();
        for (idx, app) in rec_apps.iter().enumerate() {
            let ry = top + rtop + SEC_H as f32 + idx as f32 * ROW_H as f32;
            if hover == Some(Hit::Recent(idx)) {
                draw::fill_round_rect(
                    &mut pixmap,
                    left + 6.0,
                    ry + 2.0,
                    LAUNCHER_WIDTH as f32 - 12.0,
                    ROW_H as f32 - 4.0,
                    8.0,
                    sel_bg,
                );
            }
            icons::app_tile(
                &mut pixmap,
                &icons::key_for(&app.id),
                left + 16.0,
                ry + (ROW_H as f32 - 18.0) / 2.0,
                18.0,
            );
            draw::text(
                &mut pixmap,
                left + 44.0,
                ry + (ROW_H as f32 - 18.0) / 2.0,
                LAUNCHER_WIDTH as f32 * 0.62,
                18.0,
                13.0,
                &app.name,
                fg,
            );
            let when = times.get(&app.id).map(|t| ago(*t));
            draw::text(
                &mut pixmap,
                left + LAUNCHER_WIDTH as f32 - 14.0 - 90.0,
                ry + (ROW_H as f32 - 14.0) / 2.0,
                90.0,
                14.0,
                11.0,
                when.as_deref().unwrap_or("Recent"),
                fg_dim,
            );
        }
    }

    // Start layout: WIDGETS strip between RECOMMENDED and the footer.
    let show_rows = searching || pinned_apps.is_empty();
    if !show_rows {
        let wtop = top + widgets_top(pinned_apps.len(), rec_apps.len()) as f32;
        section(&mut pixmap, left + 16.0, wtop, "WIDGETS", fg_dim);
        if !wigs.is_empty() {
            let gap = 8.0f32;
            let card_w = (LAUNCHER_WIDTH as f32 - 32.0 - gap * (wigs.len() as f32 - 1.0))
                / wigs.len() as f32;
            for (i, wig) in wigs.iter().enumerate() {
                let wx = left + 16.0 + i as f32 * (card_w + gap);
                let wy = wtop + SEC_H as f32 + 4.0;
                draw::fill_round_rect(
                    &mut pixmap,
                    wx,
                    wy,
                    card_w,
                    WIDGET_H as f32 - 10.0,
                    10.0,
                    input_bg,
                );
                icons::icon(&mut pixmap, wig.icon, wx + 9.0, wy + 8.0, 14.0, glyph);
                draw::text_bold(
                    &mut pixmap,
                    wx + 30.0,
                    wy + 6.0,
                    card_w - 36.0,
                    18.0,
                    15.0,
                    &wig.big,
                    fg,
                );
                draw::text(
                    &mut pixmap,
                    wx + 10.0,
                    wy + 26.0,
                    card_w - 18.0,
                    14.0,
                    10.0,
                    &wig.small,
                    fg_dim,
                );
                if let Some(frac) = wig.bar {
                    let bar_w = card_w - 20.0;
                    draw::fill_round_rect(
                        &mut pixmap,
                        wx + 10.0,
                        wy + WIDGET_H as f32 - 18.0,
                        bar_w,
                        3.0,
                        1.5,
                        sep,
                    );
                    draw::fill_round_rect(
                        &mut pixmap,
                        wx + 10.0,
                        wy + WIDGET_H as f32 - 18.0,
                        bar_w * frac,
                        3.0,
                        1.5,
                        sel_bg,
                    );
                }
            }
        }
    }

    // Results list (searching, or the no-pinned fallback).
    let rtop = rows_top(pinned_apps.len(), rec_apps.len(), searching) as f32;
    for (idx, row) in rows.iter().take(MAX_ROWS).enumerate() {
        if !show_rows {
            break;
        }
        let ry = top + rtop + idx as f32 * ROW_H as f32;
        if idx == state.launcher_sel.min(n_items.saturating_sub(1)) && n_items > 0 {
            draw::fill_round_rect(
                &mut pixmap,
                left + 6.0,
                ry + 2.0,
                LAUNCHER_WIDTH as f32 - 12.0,
                ROW_H as f32 - 4.0,
                8.0,
                sel_bg,
            );
        }
        icons::app_tile(
            &mut pixmap,
            &row.icon,
            left + 16.0,
            ry + (ROW_H as f32 - 18.0) / 2.0,
            18.0,
        );
        let title: String = row.title.chars().take(34).collect();
        draw::text(
            &mut pixmap,
            left + 44.0,
            ry + (ROW_H as f32 - 18.0) / 2.0,
            LAUNCHER_WIDTH as f32 * 0.52,
            18.0,
            13.0,
            &title,
            fg,
        );
        if !row.sub.is_empty() {
            let sub: String = row.sub.chars().take(24).collect();
            let sub_x = left + 48.0 + (title.chars().count() as f32 * 7.0).min(240.0);
            draw::text(
                &mut pixmap,
                sub_x,
                ry + (ROW_H as f32 - 14.0) / 2.0,
                LAUNCHER_WIDTH as f32 - (sub_x - left) - 74.0,
                14.0,
                11.0,
                &sub,
                fg_dim,
            );
        }
        draw::text(
            &mut pixmap,
            left + LAUNCHER_WIDTH as f32 - 14.0 - 60.0,
            ry + (ROW_H as f32 - 14.0) / 2.0,
            60.0,
            14.0,
            11.0,
            row.hint,
            fg_dim,
        );
    }
    if show_rows && rows.is_empty() {
        draw::text(
            &mut pixmap,
            left + 20.0,
            top + rtop + 8.0,
            LAUNCHER_WIDTH as f32 - 40.0,
            18.0,
            13.0,
            "No results",
            fg_dim,
        );
    }

    // Footer (Start-style): separator + Settings / Log out buttons.
    let fy = top + height - FOOTER_H as f32;
    draw::fill_rect(
        &mut pixmap,
        left + 1.0,
        fy,
        LAUNCHER_WIDTH as f32 - 2.0,
        1.0,
        sep,
    );
    let by = fy + (FOOTER_H as f32 - 28.0) / 2.0;
    // User avatar (initials on accent) + name — opens Settings.
    if hover == Some(Hit::Action(0)) {
        draw::fill_round_rect(
            &mut pixmap,
            left + 8.0,
            by - 2.0,
            USER_W as f32,
            32.0,
            8.0,
            sel_bg,
        );
    }
    let name = user_name();
    draw::fill_round_rect(
        &mut pixmap,
        left + 14.0,
        by,
        28.0,
        28.0,
        14.0,
        draw::accent(state.dark),
    );
    draw::text(
        &mut pixmap,
        left + 14.0 + (28.0 - initials(&name).chars().count() as f32 * 7.5) / 2.0,
        by + 7.0,
        28.0,
        14.0,
        11.0,
        &initials(&name),
        CtColor::rgba(0xFF, 0xFF, 0xFF, 0xFF),
    );
    draw::text(
        &mut pixmap,
        left + 52.0,
        by + 5.0,
        USER_W as f32 - 52.0,
        18.0,
        13.0,
        &name,
        fg,
    );
    let rx = left + LAUNCHER_WIDTH as f32 - 8.0 - 104.0;
    draw::fill_round_rect(
        &mut pixmap,
        rx,
        by,
        104.0,
        28.0,
        8.0,
        if hover == Some(Hit::Action(1)) {
            sel_bg
        } else {
            input_bg
        },
    );
    icons::icon(&mut pixmap, "sys-logout", rx + 16.0, by + 7.0, 14.0, glyph);
    draw::text(
        &mut pixmap,
        rx + 34.0,
        by + 5.0,
        88.0,
        18.0,
        12.0,
        "Log out",
        fg,
    );

    let wl_surface = layer.wl_surface().clone();
    state.set_viewport(&wl_surface, w, h);
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
    wl_surface.commit();
}

fn section(pixmap: &mut PixmapMut<'_>, x: f32, y: f32, label: &str, color: CtColor) {
    // Win11's section headers are semibold, small, and quiet.
    draw::text_bold(
        pixmap,
        x,
        y + (SEC_H as f32 - 14.0) / 2.0,
        220.0,
        14.0,
        11.0,
        label,
        color,
    );
}

/// Small magnifier glyph: circle + handle.
fn magnifier(pixmap: &mut PixmapMut<'_>, cx: f32, cy: f32, color: CtColor) {
    let c = Color::from_rgba8(color.r(), color.g(), color.b(), 0xFF);
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(c),
        anti_alias: true,
        ..Default::default()
    };
    let stroke = Stroke {
        width: 1.4,
        ..Default::default()
    };
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy - 1.0, 4.0);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, crate::draw::xf(), None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(cx + 3.0, cy + 2.0);
    pb.line_to(cx + 6.5, cy + 5.5);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, crate::draw::xf(), None);
    }
}

pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let hit = if state.launcher_search {
        let rows = state.filtered_results();
        hit_test_search(
            x,
            y,
            state.launcher_size,
            &rows,
            state.launcher_query.is_empty(),
            state.launcher_card_h,
        )
    } else {
        let np = pinned(state).len();
        let searching = !state.launcher_query.is_empty();
        let n = rows_shown(searching, np, state.filtered_results().len());
        let nr = recommended(state).len();
        hit_test(
            x,
            y,
            state.launcher_size,
            n,
            np,
            nr,
            searching,
            state.dock_position,
        )
    };
    let mut dirty = false;
    // Track the hovered cell/row/button — the draw pass highlights it.
    if state.launcher_hover != Some(hit) {
        state.launcher_hover = Some(hit);
        dirty = true;
    }
    // Result rows also move the keyboard selection like before.
    if let Hit::Item(idx) = hit {
        if state.launcher_sel != idx {
            state.launcher_sel = idx;
            dirty = true;
        }
    }
    dirty
}

pub fn key_press(state: &mut ShellState, event: KeyEvent) {
    match event.keysym {
        Keysym::Escape => {
            state.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
        }
        Keysym::Return | Keysym::KP_Enter => state.launch_selected(),
        Keysym::BackSpace => {
            state.launcher_query.pop();
            state.launcher_sel = 0;
            state.launcher_dirty = true;
        }
        Keysym::Down => {
            let n = state.filtered_results().len();
            let max = if state.launcher_search {
                SEARCH_ROWS
            } else {
                MAX_ROWS
            };
            if n > 0 {
                state.launcher_sel = (state.launcher_sel + 1).min(n.min(max) - 1);
                state.launcher_dirty = true;
            }
        }
        Keysym::Up => {
            state.launcher_sel = state.launcher_sel.saturating_sub(1);
            state.launcher_dirty = true;
        }
        Keysym::Tab if state.launcher_search => {
            // Tab flips between Search and Ask.
            match state.launcher_query.strip_prefix('?') {
                Some(rest) => state.launcher_query = rest.to_string(),
                None => state.launcher_query.insert(0, '?'),
            }
            state.launcher_sel = 0;
            state.launcher_dirty = true;
        }
        Keysym::Tab => {
            let n = state.filtered_results().len();
            if n > 0 {
                state.launcher_sel = (state.launcher_sel + 1) % n.min(MAX_ROWS);
                state.launcher_dirty = true;
            }
        }
        Keysym::Delete => {}
        _ => {
            if let Some(utf8) = event.utf8.as_deref() {
                for c in utf8.chars() {
                    if !c.is_control() {
                        state.launcher_query.push(c);
                    }
                }
                state.launcher_sel = 0;
                state.launcher_dirty = true;
            }
        }
    }
}

#[cfg(test)]
mod search_layout_tests {
    use super::*;
    use crate::search::{Kind, Row};

    #[test]
    fn start_sits_above_the_bottom_dock_and_below_the_menubar() {
        let size = (1536, 864);
        let (left, top) = box_origin(size, dock::DockPos::Bottom, 560.0);
        assert_eq!(left, (1536.0 - LAUNCHER_WIDTH as f64) / 2.0);
        assert_eq!(top + 560.0, 864.0 - dock::STRIP as f64 - START_GAP);
        let (_, top) = box_origin(size, dock::DockPos::Bottom, 900.0);
        assert_eq!(top, crate::PANEL_HEIGHT as f64 + START_GAP);
        let (left, top) = box_origin(size, dock::DockPos::Left, 560.0);
        assert_eq!(left, dock::STRIP as f64 + START_GAP);
        assert_eq!(top, crate::PANEL_HEIGHT as f64 + START_GAP);
        let (left, _) = box_origin(size, dock::DockPos::Right, 560.0);
        assert_eq!(
            left + LAUNCHER_WIDTH as f64,
            1536.0 - dock::STRIP as f64 - START_GAP
        );
    }

    fn row(kind: Kind) -> Row {
        Row {
            kind,
            title: "x".into(),
            hint: "",
            icon: String::new(),
            sub: String::new(),
        }
    }

    #[test]
    fn pill_alone_until_typing_then_grouped() {
        assert_eq!(search_layout(&[], true).height, PILL_H);
        let rows = [
            row(Kind::Setting("dock")),
            row(Kind::File("/a".into())),
            row(Kind::File("/b".into())),
        ];
        let lay = search_layout(&rows, false);
        let labels: Vec<&str> = lay.headers.iter().map(|h| h.1).collect();
        assert_eq!(labels, ["TOP HIT", "Files"]);
        assert_eq!(lay.rows.len(), 3);
        let size = (1536, 864);
        let (l, t) = search_origin(size);
        assert_eq!(
            hit_test_search(l + 100.0, t + 20.0, size, &rows, false, 0.0),
            Hit::Input
        );
        assert_eq!(
            hit_test_search(l + 100.0, t + lay.rows[2] + 5.0, size, &rows, false, 0.0),
            Hit::Item(2)
        );
        assert_eq!(
            hit_test_search(5.0, 5.0, size, &rows, false, 0.0),
            Hit::Backdrop
        );
        // A preview pane makes the card taller than its rows; clicks there stay inside.
        assert_eq!(
            hit_test_search(l + 600.0, t + 280.0, size, &rows, false, 330.0),
            Hit::List
        );
    }
}

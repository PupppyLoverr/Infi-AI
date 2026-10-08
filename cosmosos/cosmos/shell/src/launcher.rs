//! The launcher overlay: dim backdrop + a centred floating card —
//! Win11 Start's shape (search field, "Pinned" icon grid, app rows,
//! system-action footer) with macOS Spotlight's placement and Launchpad's
//! icon-grid idiom. Typing switches the card into a Spotlight results list.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{
    seat::keyboard::{KeyEvent, Keysym},
    shell::WaylandSurface,
};
use tiny_skia::{Color, PathBuilder, PixmapMut, Stroke, Transform};
use wayland_client::protocol::wl_shm;

use crate::{desktop::AppEntry, dock, draw, icons, ShellState, LAUNCHER_WIDTH};

const INPUT_H: f64 = 48.0;
const ROW_H: f64 = 36.0;
const SEC_H: f64 = 24.0;
const FOOTER_H: f64 = 44.0;
const CARD_R: f32 = 12.0;
const MAX_ROWS: usize = 5;
const GRID_COLS: usize = 6;
const CELL_H: f64 = 78.0;
const CELL_ICON: f32 = 30.0;
/// Gap between overlay top edge and the launcher box.
const TOP_PAD_FRAC: f64 = 0.16;

/// Recently launched apps shown under the pinned grid, newest first.
const MAX_REC: usize = 3;
/// Widget card strip height in the non-searching (Start) layout.
const WIDGET_H: f64 = 60.0;

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
        let id = line.trim();
        if !id.is_empty() && !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    }
    out.truncate(8);
    out
}

/// Write the launch MRU back to disk.
pub fn save_recent(recent: &[String]) {
    let Some(path) = recent_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, recent.join("\n") + "\n");
}

fn box_top(h: u32) -> f64 {
    (h as f64 * TOP_PAD_FRAC).max(48.0)
}

fn box_left(w: u32) -> f64 {
    ((w as f64 - LAUNCHER_WIDTH as f64) / 2.0).max(8.0)
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
        h += SEC_H + WIDGET_H;
    }
    h + FOOTER_H
}

fn footer_top(h: u32, n_items: usize, n_pinned: usize, n_rec: usize, searching: bool) -> f64 {
    box_top(h) + box_height(n_items, n_pinned, n_rec, searching) - FOOTER_H
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
                cache.insert(wdir.clone(), Cached { widget, at: Instant::now(), refresh });
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

pub fn hit_test(
    x: f64,
    y: f64,
    size: (u32, u32),
    n_items: usize,
    n_pinned: usize,
    n_rec: usize,
    searching: bool,
) -> Hit {
    let left = box_left(size.0);
    let top = box_top(size.1);
    let height = box_height(n_items, n_pinned, n_rec, searching);
    if x < left || x > left + LAUNCHER_WIDTH as f64 || y < top || y > top + height {
        return Hit::Backdrop;
    }
    let ry = y - top;
    if ry >= height - FOOTER_H {
        let btn_w = 104.0;
        let mid = LAUNCHER_WIDTH as f64;
        if x < left + 8.0 + btn_w {
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
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            draw::accent_soft(false),
            Color::from_rgba8(0xC4, 0xC4, 0xC8, 0xFF),
            Color::from_rgba8(0xEF, 0xEF, 0xF1, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
    }
}

pub fn draw(state: &mut ShellState) {
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

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("launcher: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    // The pool slot may still carry a previous surface's frame — the dim
    // scrim blends SrcOver, so stale bytes would show through as a dimmed
    // ghost. Clear to transparent first.
    pixmap.fill(Color::TRANSPARENT);

    // Spotlight idiom: a vignette scrim — airy near the card, deeper at
    // the corners — then the card's own soft shadow.
    draw::scrim(&mut pixmap, w, h, state.dark);

    let left = box_left(w) as f32;
    let top = box_top(h) as f32;
    let height = box_height(rows.len(), pinned_apps.len(), rec_apps.len(), searching) as f32;
    draw::shadow(
        &mut pixmap,
        left,
        top,
        LAUNCHER_WIDTH as f32,
        height,
        CARD_R,
    );
    draw::fill_round_rect(
        &mut pixmap,
        left,
        top,
        LAUNCHER_WIDTH as f32,
        height,
        CARD_R,
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
            icons::icon(
                &mut pixmap,
                &icons::key_for(&app.id),
                cx + (cell_w - CELL_ICON) / 2.0,
                cy + 12.0,
                CELL_ICON,
                icons::tint_for(&icons::key_for(&app.id), state.dark).unwrap_or(glyph),
            );
            draw::text(
                &mut pixmap,
                cx + 4.0,
                cy + 12.0 + CELL_ICON + 6.0,
                cell_w - 8.0,
                14.0,
                10.5,
                &app.name,
                fg,
            );
        }
    }

    // RECOMMENDED rows (Win11 Start) between the grid and ALL APPS.
    if !searching && !rec_apps.is_empty() {
        let rtop = rec_top(pinned_apps.len(), searching) as f32;
        section(&mut pixmap, left + 16.0, top + rtop, "RECOMMENDED", fg_dim);
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
            icons::icon(
                &mut pixmap,
                &icons::key_for(&app.id),
                left + 16.0,
                ry + (ROW_H as f32 - 18.0) / 2.0,
                18.0,
                icons::tint_for(&icons::key_for(&app.id), state.dark).unwrap_or(glyph),
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
            draw::text(
                &mut pixmap,
                left + LAUNCHER_WIDTH as f32 - 14.0 - 60.0,
                ry + (ROW_H as f32 - 14.0) / 2.0,
                60.0,
                14.0,
                11.0,
                "Recent",
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
        icons::icon(
            &mut pixmap,
            &row.icon,
            left + 16.0,
            ry + (ROW_H as f32 - 18.0) / 2.0,
            18.0,
            icons::tint_for(&row.icon, state.dark).unwrap_or(glyph),
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
    let fy = footer_top(h, rows.len(), pinned_apps.len(), rec_apps.len(), searching) as f32;
    draw::fill_rect(
        &mut pixmap,
        left + 1.0,
        fy,
        LAUNCHER_WIDTH as f32 - 2.0,
        1.0,
        sep,
    );
    let by = fy + (FOOTER_H as f32 - 28.0) / 2.0;
    draw::fill_round_rect(
        &mut pixmap,
        left + 8.0,
        by,
        104.0,
        28.0,
        8.0,
        if hover == Some(Hit::Action(0)) {
            sel_bg
        } else {
            input_bg
        },
    );
    icons::icon(
        &mut pixmap,
        "cosmos-settings",
        left + 18.0,
        by + 7.0,
        14.0,
        glyph,
    );
    draw::text(
        &mut pixmap,
        left + 38.0,
        by + 5.0,
        88.0,
        18.0,
        12.0,
        "Settings",
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
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
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
        pixmap.stroke_path(&path, &paint, &stroke, Transform::default(), None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(cx + 3.0, cy + 2.0);
    pb.line_to(cx + 6.5, cy + 5.5);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::default(), None);
    }
}

pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let np = pinned(state).len();
    let searching = !state.launcher_query.is_empty();
    let n = rows_shown(searching, np, state.filtered_results().len());
    let nr = recommended(state).len();
    let hit = hit_test(x, y, state.launcher_size, n, np, nr, searching);
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
            if n > 0 {
                state.launcher_sel = (state.launcher_sel + 1).min(n.min(MAX_ROWS) - 1);
                state.launcher_dirty = true;
            }
        }
        Keysym::Up => {
            state.launcher_sel = state.launcher_sel.saturating_sub(1);
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

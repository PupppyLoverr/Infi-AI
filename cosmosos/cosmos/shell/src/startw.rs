//! Start's widget board (spec §2.3): live data behind the two-column
//! masonry of Calendar, CPU/RAM rings, Todos, Volume and Photos.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

pub const GAP: f32 = 8.0;
pub const HEIGHT: f32 = 280.0;

pub type Rect = (f32, f32, f32, f32);

/// Card rectangles: left column Calendar over Rings, right column Todos,
/// Volume, Photos. Both columns end at `y + HEIGHT`.
pub struct Cards {
    pub calendar: Rect,
    pub rings: Rect,
    pub todos: Rect,
    pub volume: Rect,
    pub photos: Rect,
}

pub fn layout(x: f32, y: f32, w: f32) -> Cards {
    let cw = (w - GAP) / 2.0;
    let rx = x + cw + GAP;
    Cards {
        calendar: (x, y, cw, 176.0),
        rings: (x, y + 176.0 + GAP, cw, HEIGHT - 176.0 - GAP),
        todos: (rx, y, cw, 140.0),
        volume: (rx, y + 140.0 + GAP, cw, 44.0),
        photos: (rx, y + 192.0 + GAP, cw, HEIGHT - 192.0 - GAP),
    }
}

pub fn inside(r: Rect, px: f32, py: f32) -> bool {
    px >= r.0 && px < r.0 + r.2 && py >= r.1 && py < r.1 + r.3
}

// ---------------- Todos: ~/.local/share/cosmos/todo.json ----------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Todo {
    pub text: String,
    pub done: bool,
}

pub fn todo_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("cosmos/todo.json")
}

pub fn load_todos(path: &std::path::Path) -> Vec<Todo> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_todos(path: &std::path::Path, todos: &[Todo]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(todos)?)?;
    std::fs::rename(tmp, path)
}

// ---------------- CPU / RAM ----------------

static CPU_PREV: Mutex<Option<(u64, u64)>> = Mutex::new(None);

/// (total, idle) jiffies from the aggregate `cpu` line of /proc/stat.
pub fn parse_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .filter_map(|t| t.parse().ok())
        .collect();
    if v.len() < 5 {
        return None;
    }
    Some((v.iter().sum(), v[3] + v[4]))
}

/// CPU busy fraction since the previous call (0 on the first call).
pub fn cpu_fraction() -> f32 {
    let Some(now) = std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|t| parse_stat(&t))
    else {
        return 0.0;
    };
    let mut prev = CPU_PREV.lock().unwrap_or_else(|e| e.into_inner());
    let frac = match *prev {
        Some((t0, i0)) if now.0 > t0 => {
            let dt = (now.0 - t0) as f32;
            1.0 - (now.1.saturating_sub(i0)) as f32 / dt
        }
        _ => 0.0,
    };
    *prev = Some(now);
    frac.clamp(0.0, 1.0)
}

// ---------------- Calendar ----------------

pub fn today() -> (i32, u32, u32) {
    // SAFETY: time/localtime_r write only into the locals passed in.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32)
    }
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Weekday of y-m-d, Monday = 0 (Sakamoto).
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let sun0 = (y + y / 4 - y / 100 + y / 400 + T[m as usize - 1] + d as i32) % 7;
    ((sun0 + 6) % 7) as u32
}

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Monday-first month grid: leading blanks then day numbers.
pub fn month_cells(y: i32, m: u32) -> Vec<Option<u32>> {
    let lead = weekday(y, m, 1) as usize;
    let mut cells = vec![None; lead];
    cells.extend((1..=days_in_month(y, m)).map(Some));
    cells
}

// ---------------- Photos: PNGs in ~/Pictures ----------------

pub fn pictures_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Pictures")
}

pub fn photos(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        })
        .collect();
    out.sort();
    out
}

/// Slideshow index: one photo every 6 s.
pub fn slide(n: usize, secs: u64) -> usize {
    if n == 0 {
        0
    } else {
        (secs / 6) as usize % n
    }
}

// ---------------- Drawing + presses ----------------

use crate::{draw, icons};
use cosmic_text::Color as CtColor;
use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, Paint, PathBuilder, Pattern, Pixmap, PixmapMut,
    SpreadMode, Stroke, Transform,
};

pub const TODO_ROWS: usize = 4;
const TODO_TOP: f32 = 36.0;
const TODO_ROW: f32 = 25.0;
const VOL_X: f32 = 44.0;

pub struct Palette {
    pub card: Color,
    pub fg: CtColor,
    pub dim: CtColor,
    pub track: Color,
    pub accent: Color,
}

fn solid(c: CtColor) -> Color {
    Color::from_rgba8(c.r(), c.g(), c.b(), c.a())
}

fn ring(pm: &mut PixmapMut<'_>, cx: f32, cy: f32, r: f32, frac: f32, color: Color) {
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.0 {
        return;
    }
    let n = ((frac * 72.0).ceil() as usize).max(2);
    let mut pb = PathBuilder::new();
    for k in 0..=n {
        let a = -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU * k as f32 / n as f32;
        let (px, py) = (cx + r * a.cos(), cy + r * a.sin());
        if k == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(color);
    let stroke = Stroke {
        width: 5.0,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    pm.stroke_path(&path, &paint, &stroke, draw::xf(), None);
}

/// Index of the todo row under `py` (relative to the todo card), counting
/// the in-progress entry row when one is open.
fn todo_row(py: f32, editing: bool, n: usize) -> Option<usize> {
    let row = ((py - TODO_TOP) / TODO_ROW).floor();
    if row < 0.0 {
        return None;
    }
    let row = row as usize;
    let shown = n.min(TODO_ROWS - usize::from(editing));
    let row = row.checked_sub(usize::from(editing))?;
    (row < shown).then_some(row)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_board(
    pm: &mut PixmapMut<'_>,
    x: f32,
    y: f32,
    w: f32,
    todos: &[Todo],
    edit: Option<&str>,
    volume: Option<&crate::sysinfo::Volume>,
    photo: &mut Option<(PathBuf, Pixmap)>,
    p: &Palette,
) {
    let c = layout(x, y, w);
    for r in [c.calendar, c.rings, c.todos, c.volume, c.photos] {
        draw::fill_round_rect(pm, r.0, r.1, r.2, r.3, 12.0, p.card);
    }
    let white = CtColor::rgba(0xFF, 0xFF, 0xFF, 0xFF);

    // Calendar: this month, Monday first, today on the accent.
    let (yy, mm, dd) = today();
    let (cx, cy, cw, _) = c.calendar;
    let title = format!("{} {yy}", MONTHS[mm as usize - 1]);
    draw::text_bold(
        pm,
        cx + 12.0,
        cy + 10.0,
        cw - 24.0,
        18.0,
        13.0,
        &title,
        p.fg,
    );
    let col = (cw - 24.0) / 7.0;
    for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        draw::text_centered(
            pm,
            cx + 12.0 + i as f32 * col,
            cy + 34.0,
            col,
            14.0,
            10.0,
            d,
            p.dim,
        );
    }
    for (i, cell) in month_cells(yy, mm).iter().enumerate() {
        let Some(day) = cell else { continue };
        let gx = cx + 12.0 + (i % 7) as f32 * col;
        let gy = cy + 52.0 + (i / 7) as f32 * 20.0;
        let mut fg = p.fg;
        if *day == dd {
            draw::fill_round_rect(
                pm,
                gx + col / 2.0 - 9.0,
                gy - 2.0,
                18.0,
                18.0,
                9.0,
                p.accent,
            );
            fg = white;
        }
        draw::text_centered(pm, gx, gy, col, 14.0, 11.0, &day.to_string(), fg);
    }

    // CPU / Memory rings.
    let (rx, ry, rw, rh) = c.rings;
    let (total, avail) = crate::launcher::meminfo();
    let mem = if total > 0 {
        total.saturating_sub(avail) as f32 / total as f32
    } else {
        0.0
    };
    for (i, (label, f)) in [("CPU", cpu_fraction()), ("Memory", mem)]
        .iter()
        .enumerate()
    {
        let r = 24.0;
        let ccx = rx + i as f32 * rw / 2.0 + 16.0 + r;
        let ccy = ry + rh / 2.0;
        ring(pm, ccx, ccy, r, 1.0, p.track);
        ring(pm, ccx, ccy, r, *f, p.accent);
        let pct = format!("{:.0}%", f * 100.0);
        draw::text_centered(pm, ccx - r, ccy - 8.0, 2.0 * r, 16.0, 11.0, &pct, p.fg);
        draw::text(
            pm,
            ccx + r + 10.0,
            ccy - 8.0,
            rw / 2.0 - 2.0 * r - 30.0,
            16.0,
            12.0,
            label,
            p.dim,
        );
    }

    // Todos: + opens an entry row; click a box to tick, × to delete.
    let (tx, ty, tw, _) = c.todos;
    draw::text_bold(
        pm,
        tx + 12.0,
        ty + 10.0,
        tw - 60.0,
        18.0,
        13.0,
        "To do",
        p.fg,
    );
    draw::fill_round_rect(pm, tx + tw - 34.0, ty + 8.0, 22.0, 22.0, 6.0, p.track);
    draw::text_centered(pm, tx + tw - 34.0, ty + 9.0, 22.0, 18.0, 14.0, "+", p.fg);
    let mut row_y = ty + TODO_TOP;
    if let Some(buf) = edit {
        draw::fill_round_rect(pm, tx + 8.0, row_y, tw - 16.0, TODO_ROW - 3.0, 6.0, p.track);
        let shown = if buf.is_empty() {
            "New task, Enter to add"
        } else {
            buf
        };
        let col = if buf.is_empty() { p.dim } else { p.fg };
        draw::text(
            pm,
            tx + 14.0,
            row_y + 3.0,
            tw - 30.0,
            16.0,
            12.0,
            shown,
            col,
        );
        let caret = tx
            + 14.0
            + if buf.is_empty() {
                0.0
            } else {
                draw::text_width(12.0, buf).min(tw - 32.0)
            };
        draw::fill_rect(pm, caret, row_y + 4.0, 1.0, 14.0, p.accent);
        row_y += TODO_ROW;
    } else if todos.is_empty() {
        draw::text(
            pm,
            tx + 12.0,
            row_y + 3.0,
            tw - 24.0,
            16.0,
            11.0,
            "No tasks. Press + to add one.",
            p.dim,
        );
    }
    for t in todos.iter().take(TODO_ROWS - usize::from(edit.is_some())) {
        draw::fill_round_rect(
            pm,
            tx + 12.0,
            row_y + 4.0,
            14.0,
            14.0,
            4.0,
            if t.done { p.accent } else { p.track },
        );
        let col = if t.done { p.dim } else { p.fg };
        draw::text(
            pm,
            tx + 34.0,
            row_y + 3.0,
            tw - 70.0,
            16.0,
            12.0,
            &t.text,
            col,
        );
        if t.done {
            let sw = draw::text_width(12.0, &t.text).min(tw - 70.0);
            draw::fill_rect(pm, tx + 34.0, row_y + 11.0, sw, 1.0, solid(p.dim));
        }
        draw::text_centered(
            pm,
            tx + tw - 30.0,
            row_y + 2.0,
            18.0,
            16.0,
            13.0,
            "×",
            p.dim,
        );
        row_y += TODO_ROW;
    }

    // Volume: the PipeWire default sink.
    let (vx, vy, vw, vh) = c.volume;
    match volume {
        Some(v) => {
            let level = v.level.clamp(0.0, 1.0);
            let key = if v.muted { "vol-mute" } else { "vol-on" };
            icons::icon(
                pm,
                key,
                vx + 12.0,
                vy + (vh - 20.0) / 2.0,
                20.0,
                solid(p.fg),
            );
            let (sx, sw, my) = (vx + VOL_X, vw - VOL_X - 16.0, vy + vh / 2.0);
            draw::fill_round_rect(pm, sx, my - 2.0, sw, 4.0, 2.0, p.track);
            draw::fill_round_rect(pm, sx, my - 2.0, sw * level, 4.0, 2.0, p.accent);
            draw::fill_round_rect(
                pm,
                sx + sw * level - 7.0,
                my - 7.0,
                14.0,
                14.0,
                7.0,
                Color::WHITE,
            );
        }
        None => draw::text(
            pm,
            vx + 12.0,
            vy + 14.0,
            vw - 24.0,
            16.0,
            12.0,
            "No audio output",
            p.dim,
        ),
    }

    // Photos: slideshow of ~/Pictures, or a way to fill it.
    let (px, py, pw, ph) = c.photos;
    let list = photos(&pictures_dir());
    if list.is_empty() {
        *photo = None;
        draw::text(
            pm,
            px + 12.0,
            py + 12.0,
            pw - 24.0,
            16.0,
            12.0,
            "Add photos to Pictures",
            p.fg,
        );
        draw::fill_round_rect(pm, px + 12.0, py + ph - 34.0, 96.0, 24.0, 7.0, p.track);
        draw::text_centered(
            pm,
            px + 12.0,
            py + ph - 30.0,
            96.0,
            16.0,
            12.0,
            "Open Files",
            p.fg,
        );
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = &list[slide(list.len(), secs)];
    if photo.as_ref().map(|(p, _)| p) != Some(path) {
        *photo = crate::preview::thumbnail(path).map(|img| (path.clone(), img));
    }
    if let (Some((_, img)), Some(clip)) =
        (photo.as_ref(), draw::round_rect_path(px, py, pw, ph, 12.0))
    {
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let s = (pw / iw).max(ph / ih);
        let t = Transform::from_row(
            s,
            0.0,
            0.0,
            s,
            px + (pw - iw * s) / 2.0,
            py + (ph - ih * s) / 2.0,
        );
        let paint = Paint {
            anti_alias: true,
            shader: Pattern::new(
                img.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                1.0,
                t,
            ),
            ..Paint::default()
        };
        pm.fill_path(&clip, &paint, FillRule::Winding, draw::xf(), None);
    }
}

fn save(todos: &[Todo]) {
    if let Err(e) = save_todos(&todo_path(), todos) {
        tracing::warn!("todo.json: {e}");
    }
}

/// A press at (px, py) relative to the board's top-left, board `w` wide.
/// Returns true when Start should close (an app was opened).
pub fn press(state: &mut crate::ShellState, w: f32, px: f32, py: f32) -> bool {
    let c = layout(0.0, 0.0, w);
    if inside(c.todos, px, py) {
        let (tx, ty, tw, _) = c.todos;
        if px >= tx + tw - 38.0 && py < ty + TODO_TOP {
            state.start_todo_edit = Some(String::new());
            return false;
        }
        let editing = state.start_todo_edit.is_some();
        let list = state
            .start_todos
            .get_or_insert_with(|| load_todos(&todo_path()));
        if let Some(i) = todo_row(py - ty, editing, list.len()) {
            if px >= tx + tw - 36.0 {
                list.remove(i);
            } else {
                list[i].done = !list[i].done;
            }
            save(list);
        }
    } else if inside(c.volume, px, py) {
        let (vx, _, vw, _) = c.volume;
        if px < vx + VOL_X {
            let _ = std::process::Command::new("wpctl")
                .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
                .status();
            if let Some(v) = state.sysinfo.volume.as_mut() {
                v.muted = !v.muted;
            }
        } else if let Some(v) = state.sysinfo.volume.as_mut() {
            let level = ((px - vx - VOL_X) / (vw - VOL_X - 16.0)).clamp(0.0, 1.0);
            let _ = std::process::Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{level:.2}")])
                .status();
            v.level = level;
        }
    } else if inside(c.photos, px, py) && photos(&pictures_dir()).is_empty() {
        let dir = pictures_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = std::process::Command::new("cosmos-files").arg(&dir).spawn() {
            tracing::warn!("open Pictures: {e}");
        }
        return true;
    }
    false
}

/// Key input while the todo entry row is open. Returns false when the
/// entry row is not open (the key belongs to Start's field).
pub fn edit_key(state: &mut crate::ShellState, key: EditKey) -> bool {
    let Some(buf) = state.start_todo_edit.as_mut() else {
        return false;
    };
    match key {
        EditKey::Char(ch) => buf.push(ch),
        EditKey::Back => {
            buf.pop();
        }
        EditKey::Cancel => state.start_todo_edit = None,
        EditKey::Enter => {
            let text = buf.trim().to_string();
            state.start_todo_edit = None;
            if !text.is_empty() {
                let list = state
                    .start_todos
                    .get_or_insert_with(|| load_todos(&todo_path()));
                list.insert(0, Todo { text, done: false });
                save(list);
            }
        }
    }
    true
}

pub enum EditKey {
    Char(char),
    Back,
    Enter,
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_maths() {
        assert_eq!(weekday(2026, 10, 8), 3); // Thursday
        assert_eq!(weekday(2024, 2, 29), 3);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        let c = month_cells(2026, 10);
        assert_eq!(c.iter().take_while(|d| d.is_none()).count(), 3);
        assert_eq!(c.last(), Some(&Some(31)));
    }

    #[test]
    fn stat_and_layout() {
        let s = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2 3 4\n";
        assert_eq!(parse_stat(s), Some((1000, 850)));
        let c = layout(0.0, 0.0, 728.0);
        assert_eq!(c.rings.1 + c.rings.3, HEIGHT);
        assert_eq!(c.photos.1 + c.photos.3, HEIGHT);
        assert!(inside(c.todos, 400.0, 10.0) && !inside(c.todos, 10.0, 10.0));
        assert_eq!(slide(3, 13), 2);
        assert_eq!(todo_row(TODO_TOP + 1.0, false, 3), Some(0));
        assert_eq!(todo_row(TODO_TOP + 1.0, true, 3), None);
        assert_eq!(todo_row(TODO_TOP + TODO_ROW + 1.0, true, 3), Some(0));
        assert_eq!(todo_row(TODO_TOP + 3.0 * TODO_ROW + 1.0, false, 3), None);
        assert_eq!(todo_row(5.0, false, 3), None);
    }

    #[test]
    fn todos_round_trip() {
        let d = std::env::temp_dir().join(format!("cosmos-todo-{}", std::process::id()));
        let p = d.join("cosmos/todo.json");
        let t = vec![Todo {
            text: "Ship §2.3".into(),
            done: false,
        }];
        save_todos(&p, &t).unwrap();
        assert_eq!(load_todos(&p), t);
        assert!(load_todos(&d.join("missing.json")).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }
}

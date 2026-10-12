//! Desktop icons: ~/Desktop as a snapped grid down the right edge, on a
//! Bottom layer so windows cover it. Its input region is just the icon
//! cells, so bare-desktop presses still reach the compositor. Drag moves
//! an icon to the nearest cell (swapping with any occupant); double-click
//! opens it. Positions persist per name in ~/.config/cosmos/desktop-icons.json.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{compositor::Region, shell::WaylandSurface};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, icons, ShellState};

pub const CELL_W: u32 = 96;
pub const CELL_H: u32 = 92;
pub const COLS: u32 = 3;
pub const MARGIN: i32 = 12;
const ICON: f32 = 48.0;
const DOUBLE: Duration = Duration::from_millis(400);
const DRAG_SLOP: f64 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    /// (column from the right edge, row from the top).
    pub cell: (u32, u32),
}

#[derive(Debug, Clone, Copy)]
pub struct Drag {
    item: usize,
    grab: (f64, f64),
    pos: (f64, f64),
    moved: bool,
    /// The press serial, for handing the drag to the compositor.
    serial: u32,
}

#[derive(Debug, Default)]
pub struct Desk {
    pub items: Vec<Item>,
    pub selected: Option<usize>,
    drag: Option<Drag>,
    last_click: Option<(usize, Instant)>,
    mtime: Option<SystemTime>,
}

fn rows(h: u32) -> u32 {
    (h / CELL_H).max(1)
}

/// Top-left of a cell in surface-local logical px.
fn cell_px((col, row): (u32, u32)) -> (f64, f64) {
    (
        ((COLS - 1 - col.min(COLS - 1)) * CELL_W) as f64,
        (row * CELL_H) as f64,
    )
}

/// Saved cells win when valid and unique; the rest fill free cells
/// top-to-bottom from the right-hand column. `None` = no room left.
pub fn place(
    names: &[String],
    saved: &HashMap<String, (u32, u32)>,
    rows: u32,
) -> Vec<Option<(u32, u32)>> {
    let mut taken = HashSet::new();
    let mut out: Vec<Option<(u32, u32)>> = names
        .iter()
        .map(|n| {
            saved
                .get(n)
                .copied()
                .filter(|c| c.0 < COLS && c.1 < rows && taken.insert(*c))
        })
        .collect();
    for slot in out.iter_mut().filter(|s| s.is_none()) {
        let free = (0..COLS)
            .flat_map(|c| (0..rows).map(move |r| (c, r)))
            .find(|c| !taken.contains(c));
        if let Some(c) = free {
            taken.insert(c);
            *slot = Some(c);
        }
    }
    out
}

/// Nearest cell to an icon whose cell top-left is at (x, y).
pub fn snap(x: f64, y: f64, rows: u32) -> (u32, u32) {
    let from_left = ((x + CELL_W as f64 / 2.0) / CELL_W as f64)
        .floor()
        .clamp(0.0, (COLS - 1) as f64) as u32;
    let row = ((y + CELL_H as f64 / 2.0) / CELL_H as f64)
        .floor()
        .clamp(0.0, (rows - 1) as f64) as u32;
    (COLS - 1 - from_left, row)
}

fn store_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("cosmos/desktop-icons.json"))
}

fn load_saved() -> HashMap<String, (u32, u32)> {
    let Some(v) = store_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    else {
        return HashMap::new();
    };
    v.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, c)| {
            let c = c.as_array()?;
            Some((
                k.clone(),
                (c.first()?.as_u64()? as u32, c.get(1)?.as_u64()? as u32),
            ))
        })
        .collect()
}

fn save(items: &[Item]) {
    let Some(path) = store_path() else {
        return;
    };
    let map: serde_json::Map<String, serde_json::Value> = items
        .iter()
        .map(|i| (i.name.clone(), serde_json::json!([i.cell.0, i.cell.1])))
        .collect();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::Value::Object(map).to_string());
}

pub fn rescan(state: &mut ShellState) {
    let dir = crate::desktop_dir();
    state.desk.mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
    let mut entries: Vec<(String, PathBuf, bool)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            // is_dir follows symlinks, so the Projects alias is a folder.
            (!name.starts_with('.')).then(|| (name, path.clone(), path.is_dir()))
        })
        .collect();
    entries.sort_by_key(|e| e.0.to_lowercase());
    let names: Vec<String> = entries.iter().map(|e| e.0.clone()).collect();
    let cells = place(&names, &load_saved(), rows(state.desk_size.1));
    let selected = state
        .desk
        .selected
        .and_then(|i| state.desk.items.get(i))
        .map(|i| i.name.clone());
    state.desk.items = entries
        .into_iter()
        .zip(cells)
        .filter_map(|((name, path, dir), cell)| {
            Some(Item {
                name,
                path,
                dir,
                cell: cell?,
            })
        })
        .collect();
    state.desk.selected = selected.and_then(|n| state.desk.items.iter().position(|i| i.name == n));
    state.desk.drag = None;
    state.desk.last_click = None;
}

/// Rescans when ~/Desktop changed; true when a repaint is needed.
pub fn refresh(state: &mut ShellState) -> bool {
    let m = std::fs::metadata(crate::desktop_dir())
        .and_then(|m| m.modified())
        .ok();
    if m == state.desk.mtime || state.desk.drag.is_some_and(|d| d.moved) {
        return false;
    }
    rescan(state);
    true
}

fn hit(items: &[Item], x: f64, y: f64) -> Option<usize> {
    items.iter().position(|i| {
        let (cx, cy) = cell_px(i.cell);
        x >= cx && x < cx + CELL_W as f64 && y >= cy && y < cy + CELL_H as f64
    })
}

fn open(item: &Item) {
    let path = item.path.to_string_lossy().into_owned();
    if item.dir {
        crate::spawn_quiet("cosmos-files", &[&path]);
    } else if std::process::Command::new("xdg-open")
        .arg(&item.path)
        .spawn()
        .is_err()
    {
        crate::spawn_quiet("cosmos-editor", &[&path]);
    }
}

pub fn press(state: &mut ShellState, x: f64, y: f64, serial: u32) -> bool {
    let Some(i) = hit(&state.desk.items, x, y) else {
        let was = state.desk.selected.take();
        return was.is_some();
    };
    let now = Instant::now();
    let double = matches!(state.desk.last_click,
        Some((j, t)) if j == i && now.duration_since(t) < DOUBLE);
    state.desk.selected = Some(i);
    if double {
        state.desk.last_click = None;
        state.desk.drag = None;
        open(&state.desk.items[i]);
    } else {
        state.desk.last_click = Some((i, now));
        let (cx, cy) = cell_px(state.desk.items[i].cell);
        state.desk.drag = Some(Drag {
            item: i,
            grab: (x - cx, y - cy),
            pos: (x, y),
            moved: false,
            serial,
        });
    }
    true
}

pub fn motion(state: &mut ShellState, x: f64, y: f64) -> bool {
    let Some(d) = state.desk.drag.as_mut() else {
        return false;
    };
    if !d.moved && (x - d.pos.0).hypot(y - d.pos.1) < DRAG_SLOP {
        return false;
    }
    d.moved = true;
    d.pos = (x, y);
    true
}

/// A moved icon dragged past the desktop column leaves as a file drag
/// (onto the dock's Files or Editor): returns its path and press serial.
pub fn take_outbound(state: &mut ShellState, x: f64, y: f64) -> Option<(PathBuf, u32)> {
    let d = state.desk.drag.filter(|d| d.moved)?;
    let (w, h) = (state.desk_size.0 as f64, state.desk_size.1 as f64);
    if (0.0..w).contains(&x) && (0.0..h).contains(&y) {
        return None;
    }
    state.desk.drag = None;
    state.desk.last_click = None;
    let item = state.desk.items.get(d.item)?;
    Some((item.path.clone(), d.serial))
}

pub fn release(state: &mut ShellState) -> bool {
    let Some(d) = state.desk.drag.take() else {
        return false;
    };
    if !d.moved {
        return false;
    }
    state.desk.last_click = None;
    let target = snap(
        d.pos.0 - d.grab.0,
        d.pos.1 - d.grab.1,
        rows(state.desk_size.1),
    );
    let old = state.desk.items[d.item].cell;
    if let Some(j) = state.desk.items.iter().position(|it| it.cell == target) {
        state.desk.items[j].cell = old;
    }
    state.desk.items[d.item].cell = target;
    save(&state.desk.items);
    true
}

fn paint_item(
    pixmap: &mut PixmapMut<'_>,
    item: &Item,
    (cx, cy): (f32, f32),
    sel: bool,
    dark: bool,
) {
    let ix = cx + (CELL_W as f32 - ICON) / 2.0;
    let iy = cy + 6.0;
    let tint = if dark { 0xFF } else { 0x00 };
    if sel {
        let c = Color::from_rgba8(tint, tint, tint, if dark { 0x26 } else { 0x1A });
        draw::fill_round_rect(pixmap, ix - 6.0, iy - 4.0, ICON + 12.0, ICON + 8.0, 10.0, c);
    }
    if item.dir {
        icons::app_tile(pixmap, "cosmos-files", ix, iy, ICON);
    } else {
        // A paper sheet with the document glyph and the file's extension.
        let (px, pw) = (ix + 7.0, ICON - 14.0);
        draw::fill_round_rect(
            pixmap,
            px,
            iy,
            pw,
            ICON,
            5.0,
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xFF),
        );
        draw::stroke_round_rect(
            pixmap,
            px + 0.5,
            iy + 0.5,
            pw - 1.0,
            ICON - 1.0,
            5.0,
            1.0,
            Color::from_rgba8(0x00, 0x00, 0x00, 0x24),
        );
        let grey = Color::from_rgba8(0x6E, 0x6E, 0x76, 0xFF);
        icons::icon(
            pixmap,
            "search-doc",
            px + (pw - 18.0) / 2.0,
            iy + 7.0,
            18.0,
            grey,
        );
        let ext: String = item
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_uppercase().chars().take(4).collect())
            .unwrap_or_default();
        if !ext.is_empty() {
            let ew = draw::text_width(9.0, &ext);
            let ec = CtColor::rgba(0x6E, 0x6E, 0x76, 0xFF);
            draw::text_bold(
                pixmap,
                px + (pw - ew) / 2.0,
                iy + ICON - 17.0,
                pw,
                12.0,
                9.0,
                &ext,
                ec,
            );
        }
    }
    let max = CELL_W as f32 - 8.0;
    let lines = label_lines(&item.name, max, |t| draw::text_width(12.0, t));
    let ly = iy + ICON + 6.0;
    // Labels sit on the wallpaper: a 1px contrasting shadow keeps them legible.
    let (fg, shadow) = if dark {
        (
            CtColor::rgba(0xFF, 0xFF, 0xFF, 0xFF),
            CtColor::rgba(0x00, 0x00, 0x00, 0x99),
        )
    } else {
        (
            CtColor::rgba(0x16, 0x16, 0x19, 0xFF),
            CtColor::rgba(0xFF, 0xFF, 0xFF, 0x80),
        )
    };
    for (n, line) in lines.iter().enumerate() {
        let tw = draw::text_width(12.0, line).min(max);
        let lx = cx + (CELL_W as f32 - tw) / 2.0;
        let y = ly + n as f32 * LINE_H;
        if sel {
            let c = Color::from_rgba8(tint, tint, tint, if dark { 0x33 } else { 0x1F });
            draw::fill_round_rect(pixmap, lx - 5.0, y - 1.0, tw + 10.0, LINE_H + 1.0, 5.0, c);
        }
        draw::text(pixmap, lx, y + 1.0, max, LINE_H, 12.0, line, shadow);
        draw::text(pixmap, lx, y, max, LINE_H, 12.0, line, fg);
    }
}

const LINE_H: f32 = 15.0;

/// Splits a label into at most two lines that each fit `max`: breaks at
/// the last space that fits, else mid-word; an overlong rest ends in "…".
fn label_lines(name: &str, max: f32, width: impl Fn(&str) -> f32) -> Vec<String> {
    if width(name) <= max {
        return vec![name.to_string()];
    }
    let chars: Vec<char> = name.chars().collect();
    let fits = |n: usize| width(&chars[..n].iter().collect::<String>()) <= max;
    let mut cut = (1..=chars.len())
        .take_while(|&n| fits(n))
        .last()
        .unwrap_or(1);
    if let Some(sp) = chars[..cut].iter().rposition(|c| *c == ' ') {
        if sp > 0 {
            cut = sp;
        }
    }
    let first: String = chars[..cut].iter().collect();
    let rest: String = chars[cut..]
        .iter()
        .collect::<String>()
        .trim_start()
        .to_string();
    let second = if width(&rest) <= max {
        rest
    } else {
        let rc: Vec<char> = rest.chars().collect();
        let keep = (0..rc.len())
            .take_while(|&n| width(&format!("{}…", rc[..=n].iter().collect::<String>())) <= max)
            .last()
            .map_or(0, |n| n + 1);
        format!("{}…", rc[..keep].iter().collect::<String>())
    };
    vec![first, second]
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.desk_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.desk_surface.clone() else {
        return;
    };
    let (pw, ph) = draw::phys(w, h);
    let Ok((buffer, canvas)) = state.pool.create_buffer(
        pw as i32,
        ph as i32,
        pw as i32 * 4,
        wl_shm::Format::Abgr8888,
    ) else {
        tracing::warn!("desktop icons: pool create_buffer failed");
        return;
    };
    canvas.fill(0);
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    let desk = &state.desk;
    let dragged = desk.drag.filter(|d| d.moved);
    for (i, item) in desk.items.iter().enumerate() {
        if dragged.is_some_and(|d| d.item == i) {
            continue;
        }
        let (cx, cy) = cell_px(item.cell);
        paint_item(
            &mut pixmap,
            item,
            (cx as f32, cy as f32),
            desk.selected == Some(i),
            state.dark,
        );
    }
    // The dragged icon paints last, under the pointer.
    if let Some(d) = dragged {
        let at = ((d.pos.0 - d.grab.0) as f32, (d.pos.1 - d.grab.1) as f32);
        paint_item(&mut pixmap, &desk.items[d.item], at, true, state.dark);
    }

    let wl_surface = layer.wl_surface().clone();
    state.set_viewport(&wl_surface, w, h);
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
    if let Ok(region) = Region::new(&state.compositor_state) {
        for item in &state.desk.items {
            let (cx, cy) = cell_px(item.cell);
            region.add(cx as i32, cy as i32, CELL_W as i32, CELL_H as i32);
        }
        wl_surface.set_input_region(Some(region.wl_region()));
    }
    wl_surface.commit();
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_labels_wrap_to_two_lines() {
        let w = |t: &str| t.chars().count() as f32 * 7.0;
        assert_eq!(super::label_lines("notes.txt", 88.0, w), vec!["notes.txt"]);
        assert_eq!(
            super::label_lines("Welcome to CosmosOS.md", 88.0, w),
            vec!["Welcome to", "CosmosOS.md"]
        );
        let l = super::label_lines("Welcome to a very long document name.md", 88.0, w);
        assert_eq!(l[0], "Welcome to");
        assert!(l[1].ends_with('…') && w(&l[1]) <= 88.0);
    }

    use super::*;

    #[test]
    fn saved_cells_win_and_the_rest_fill_down_the_right_column() {
        let names: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let saved = HashMap::from([("b".to_string(), (0, 0)), ("c".to_string(), (9, 9))]);
        assert_eq!(
            place(&names, &saved, 4),
            vec![Some((0, 1)), Some((0, 0)), Some((0, 2))]
        );
        let many: Vec<String> = (0..7).map(|i| i.to_string()).collect();
        let cells = place(&many, &HashMap::new(), 2);
        assert_eq!(cells[2], Some((1, 0)));
        assert_eq!(cells[6], None);
    }

    #[test]
    fn snap_rounds_to_the_nearest_cell_and_clamps() {
        let (rx, _) = cell_px((0, 0));
        assert_eq!(snap(rx + 10.0, 30.0, 5), (0, 0));
        assert_eq!(snap(rx - 60.0, CELL_H as f64 * 2.4, 5), (1, 2));
        assert_eq!(snap(-500.0, 9999.0, 5), (COLS - 1, 4));
    }
}

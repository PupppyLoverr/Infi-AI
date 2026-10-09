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

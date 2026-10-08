//! Background file index for the launcher's Search mode. A thread walks
//! the user's real files (bounded: no hidden dirs, depth ≤ 3, 20k cap)
//! and hands the shell a (name, path) list it can substring-match
//! instantly. Refreshes at most once a minute when the launcher opens.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ShellState;

/// Re-index at most this often — the walk is background work but the
/// whole point of the budgets is not doing it needlessly.
const REFRESH_AFTER: Duration = Duration::from_secs(60);
const MAX_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 3;

const INDEX_DIRS: &[&str] = &[
    "Documents",
    "Downloads",
    "Desktop",
    "Pictures",
    "Music",
    "Videos",
];

/// Kick a refresh if the index is stale (or never built).
pub fn maybe_refresh(state: &mut ShellState) {
    if state.index_refreshed.elapsed() < REFRESH_AFTER {
        return;
    }
    state.index_refreshed = Instant::now();
    let tx = state.index_tx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(scan());
    });
}

fn scan() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return out;
    };
    // Top-level non-hidden files of $HOME, then the standard dirs.
    walk(&home, 0, true, &mut out);
    for name in INDEX_DIRS {
        walk(&home.join(name), 0, false, &mut out);
    }
    walk(&home.join(".config/cosmos"), 0, false, &mut out);
    out
}

fn walk(dir: &Path, depth: usize, files_only: bool, out: &mut Vec<(String, String)>) {
    if out.len() >= MAX_ENTRIES {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        if out.len() >= MAX_ENTRIES {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_file() {
            out.push((name, path.to_string_lossy().to_string()));
        } else if meta.is_dir() && !files_only && depth < MAX_DEPTH {
            walk(&path, depth + 1, false, out);
        }
    }
}

/// Case-insensitive name matches, best first: name starts-with beats
/// name-contains beats path-contains. Bounded to `limit`.
pub fn query<'a>(
    index: &'a [(String, String)],
    q: &str,
    limit: usize,
) -> Vec<&'a (String, String)> {
    let q = q.to_lowercase();
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    let mut path_hits = Vec::new();
    for e in index {
        let name = e.0.to_lowercase();
        if name.starts_with(&q) {
            starts.push(e);
        } else if name.contains(&q) {
            contains.push(e);
        } else if e.1.to_lowercase().contains(&q) {
            path_hits.push(e);
        }
        if starts.len() + contains.len() + path_hits.len() > limit * 8 {
            break;
        }
    }
    starts.extend(contains);
    starts.extend(path_hits);
    starts.truncate(limit);
    starts
}

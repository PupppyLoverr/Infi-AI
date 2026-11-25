//! The notch's file shelf (pearos-notch's "Tray"): drop files onto the notch
//! to park them, click one to open it, remove it when done. Only paths are
//! kept, nothing is copied, so the shelf costs no disk space; a file that
//! moves or disappears drops off the list.
//!
//! Stored one path per line in `$XDG_DATA_HOME/solyra/shelf.list`.

use std::path::{Path, PathBuf};

/// How many files the shelf holds; the oldest goes when a new one arrives.
pub const MAX: usize = 8;

fn store() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("solyra/shelf.list"))
}

pub fn load() -> Vec<PathBuf> {
    load_from(store().as_deref())
}

fn load_from(path: Option<&Path>) -> Vec<PathBuf> {
    let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    text.lines()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.exists())
        .take(MAX)
        .collect()
}

pub fn save(items: &[PathBuf]) {
    let Some(path) = store() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = items
        .iter()
        .filter_map(|p| p.to_str())
        .filter(|s| !s.contains('\n'))
        .map(|s| format!("{s}\n"))
        .collect();
    let _ = std::fs::write(path, text);
}

/// Add `new` to the front, without duplicates, keeping at most `MAX`.
pub fn add(items: &mut Vec<PathBuf>, new: impl IntoIterator<Item = PathBuf>) {
    for p in new {
        items.retain(|q| q != &p);
        items.insert(0, p);
    }
    items.truncate(MAX);
}

/// File paths from a `text/uri-list` drop (`file://` URIs, percent-decoded).
/// Anything that is not a local file is ignored.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://"))
        // file://host/path: only the local host (empty or "localhost").
        .filter_map(|rest| {
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            path.starts_with('/').then(|| percent_decode(path))
        })
        .map(PathBuf::from)
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What the shelf shows for a path: its file name.
pub fn label(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// A shell command that opens the file with its default app, safely quoted.
pub fn open_command(p: &Path) -> String {
    let quoted = p.display().to_string().replace('\'', "'\\''");
    format!("xdg-open '{quoted}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_lists_become_local_paths() {
        let v = parse_uri_list(
            "# comment\r\nfile:///home/me/My%20Report.pdf\r\nfile://localhost/tmp/a.txt\nhttps://x.org/y\nfile://otherhost/z\n",
        );
        assert_eq!(
            v,
            vec![PathBuf::from("/home/me/My Report.pdf"), PathBuf::from("/tmp/a.txt")]
        );
        assert_eq!(parse_uri_list("file:///a%2"), vec![PathBuf::from("/a%2")]);
    }

    #[test]
    fn newest_first_no_duplicates_bounded() {
        let mut v = Vec::new();
        add(&mut v, ["/a", "/b"].map(PathBuf::from));
        add(&mut v, [PathBuf::from("/a")]);
        assert_eq!(v, ["/a", "/b"].map(PathBuf::from));
        add(&mut v, (0..20).map(|i| PathBuf::from(format!("/f{i}"))));
        assert_eq!(v.len(), MAX);
        assert_eq!(v[0], PathBuf::from("/f19"));
    }

    #[test]
    fn missing_files_drop_off_and_commands_are_quoted() {
        let dir = std::env::temp_dir().join(format!("solyra-shelf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("it's here.txt");
        std::fs::write(&real, "x").unwrap();
        let list = dir.join("shelf.list");
        std::fs::write(&list, format!("{}\n/definitely/not/here\nrelative\n", real.display())).unwrap();
        assert_eq!(load_from(Some(&list)), vec![real.clone()]);
        assert_eq!(label(&real), "it's here.txt");
        assert!(open_command(&real).ends_with("it'\\''s here.txt'"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

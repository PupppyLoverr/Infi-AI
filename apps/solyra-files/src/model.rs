//! File-system model for Solyra Files. The UI stays thin; navigation,
//! sorting, rename validation, trash, recents and stars are testable here.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub hidden: bool,
    pub size: u64,
    pub modified: u64,
}

pub fn read_entries(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let path = item.path();
        let meta = item.metadata().or_else(|_| fs::symlink_metadata(&path))?;
        let name = item.file_name().to_string_lossy().into_owned();
        let hidden = name.starts_with('.');
        if hidden && !show_hidden {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        entries.push(Entry {
            name,
            path,
            is_dir: meta.is_dir(),
            is_symlink: meta.file_type().is_symlink(),
            hidden,
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified,
        });
    }
    sort_entries(&mut entries);
    Ok(entries)
}

/// Folders first, then case-insensitive names with a stable tie-break.
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub fn filtered(entries: &[Entry], query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|e| e.name.to_lowercase().contains(&q))
        .cloned()
        .collect()
}

pub fn validate_new_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("The name cannot be empty.".into());
    }
    if name == "." || name == ".." {
        return Err("That name is reserved.".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("A file name cannot contain a path separator.".into());
    }
    if name.chars().any(char::is_control) {
        return Err("A file name cannot contain control characters.".into());
    }
    Ok(name)
}

pub fn rename_entry(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let name = validate_new_name(new_name)?;
    let Some(parent) = path.parent() else {
        return Err("This item has no parent folder.".into());
    };
    let dest = parent.join(name);
    if dest == path {
        return Ok(dest);
    }
    if dest.exists() {
        return Err(format!("An item named {name} already exists."));
    }
    fs::rename(path, &dest).map_err(|e| format!("Could not rename the item: {e}"))?;
    Ok(dest)
}

fn data_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SOLYRA_FILES_DATA") {
        return Some(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(dir).join("solyra-files"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/solyra-files"))
}

fn read_list(root: &Path, name: &str) -> Vec<PathBuf> {
    fs::read_to_string(root.join(name))
        .ok()
        .into_iter()
        .flat_map(|text| text.lines().map(PathBuf::from).collect::<Vec<_>>())
        .collect()
}

fn write_list(root: &Path, name: &str, items: &[PathBuf]) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let text = items
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(root.join(name), format!("{text}\n"))
}

pub fn starred() -> Vec<PathBuf> {
    data_root()
        .map(|root| {
            read_list(&root, "starred")
                .into_iter()
                .filter(|p| p.exists())
                .collect()
        })
        .unwrap_or_default()
}

pub fn toggle_star(path: &Path) -> io::Result<bool> {
    let Some(root) = data_root() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "HOME is not set"));
    };
    let mut items = read_list(&root, "starred");
    let now_starred = if let Some(pos) = items.iter().position(|p| p == path) {
        items.remove(pos);
        false
    } else {
        items.insert(0, path.to_path_buf());
        true
    };
    write_list(&root, "starred", &items)?;
    Ok(now_starred)
}

pub fn record_recent(path: &Path) {
    let Some(root) = data_root() else {
        return;
    };
    let mut items = read_list(&root, "recent");
    items.retain(|p| p != path);
    items.insert(0, path.to_path_buf());
    items.truncate(30);
    let _ = write_list(&root, "recent", &items);
}

pub fn recent() -> Vec<PathBuf> {
    data_root()
        .map(|root| {
            read_list(&root, "recent")
                .into_iter()
                .filter(|p| p.exists())
                .collect()
        })
        .unwrap_or_default()
}

pub const TAG_COLORS: [&str; 6] = ["red", "orange", "yellow", "green", "blue", "purple"];

fn read_tags(root: &Path) -> Vec<(String, PathBuf)> {
    fs::read_to_string(root.join("tags"))
        .ok()
        .into_iter()
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| {
                    let (color, path) = line.split_once('\t')?;
                    Some((color.to_string(), PathBuf::from(percent_decode(path))))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn write_tags(root: &Path, items: &[(String, PathBuf)]) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let text = items
        .iter()
        .map(|(color, path)| format!("{color}\t{}", percent_encode_path(path)))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(root.join("tags"), format!("{text}\n"))
}

pub fn paths_tagged(color: &str) -> Vec<PathBuf> {
    data_root()
        .map(|root| {
            read_tags(&root)
                .into_iter()
                .filter(|(tag, path)| tag == color && path.exists())
                .map(|(_, path)| path)
                .collect()
        })
        .unwrap_or_default()
}

pub fn tag_for(path: &Path) -> Option<String> {
    data_root().and_then(|root| {
        read_tags(&root)
            .into_iter()
            .find_map(|(color, item)| (item == path).then_some(color))
    })
}

/// Cycle the Finder-style colour tag on a path. Returns the new tag, or `None`
/// when the path has come back around to untagged.
pub fn cycle_tag(path: &Path) -> io::Result<Option<String>> {
    let Some(root) = data_root() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "HOME is not set"));
    };
    let mut items = read_tags(&root);
    let current = items
        .iter()
        .position(|(_, item)| item == path)
        .map(|pos| items.remove(pos).0);
    let next = match current.as_deref() {
        None => Some(TAG_COLORS[0]),
        Some(color) => TAG_COLORS
            .iter()
            .position(|c| *c == color)
            .and_then(|pos| TAG_COLORS.get(pos + 1).copied()),
    };
    if let Some(color) = next {
        items.insert(0, (color.to_string(), path.to_path_buf()));
    }
    write_tags(&root, &items)?;
    Ok(next.map(str::to_string))
}

pub fn trash_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(dir).join("Trash"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/Trash"))
}

fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.to_string(), String::new()),
    };
    (2..10_000)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

fn percent_encode_path(path: &Path) -> String {
    let mut out = String::new();
    for byte in path.as_os_str().to_string_lossy().as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn copy_tree(src: &Path, dest: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dest)?;
        for item in fs::read_dir(src)? {
            let item = item?;
            copy_tree(&item.path(), &dest.join(item.file_name()))?;
        }
    } else {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dest)?;
    }
    Ok(())
}

fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Move an item to the freedesktop Trash. The `.trashinfo` file means the
/// item can be restored by any ordinary Linux file manager, not just us.
pub fn move_to_trash(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err("The item no longer exists.".into());
    }
    let Some(root) = trash_dir() else {
        return Err("Cannot find the trash folder because HOME is not set.".into());
    };
    let files = root.join("files");
    let info = root.join("info");
    fs::create_dir_all(&files)
        .and_then(|()| fs::create_dir_all(&info))
        .map_err(|e| format!("Could not prepare the trash: {e}"))?;
    let name = path
        .file_name()
        .ok_or_else(|| "This item has no file name.".to_string())?
        .to_string_lossy()
        .into_owned();
    let dest = unique_destination(&files, &name);
    let original = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if fs::rename(path, &dest).is_err() {
        copy_tree(path, &dest)
            .and_then(|()| remove_tree(path))
            .map_err(|e| format!("Could not move the item to the trash: {e}"))?;
    }
    let info_name = format!(
        "{}.trashinfo",
        dest.file_name().unwrap_or_default().to_string_lossy()
    );
    let deletion_date = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S");
    let body = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode_path(&original),
        deletion_date
    );
    fs::write(info.join(info_name), body).map_err(|e| {
        format!("The item was trashed, but its restore information could not be written: {e}")
    })?;
    Ok(dest)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Put a trashed item back where it came from, using its `.trashinfo`.
pub fn restore_from_trash(path: &Path) -> Result<PathBuf, String> {
    let Some(root) = trash_dir() else {
        return Err("Cannot find the trash folder because HOME is not set.".into());
    };
    let name = path
        .file_name()
        .ok_or_else(|| "This item has no file name.".to_string())?
        .to_string_lossy();
    let info_path = root.join("info").join(format!("{name}.trashinfo"));
    let info = fs::read_to_string(&info_path)
        .map_err(|e| format!("Could not read the item's restore information: {e}"))?;
    let encoded = info
        .lines()
        .find_map(|line| line.strip_prefix("Path="))
        .ok_or_else(|| "The restore information has no original path.".to_string())?;
    let dest = PathBuf::from(percent_decode(encoded));
    if dest.exists() {
        return Err("An item already exists at the original location.".into());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not recreate the original folder: {e}"))?;
    }
    fs::rename(path, &dest).map_err(|e| format!("Could not restore the item: {e}"))?;
    let _ = fs::remove_file(info_path);
    Ok(dest)
}

pub fn open_with_system(path: &Path) -> io::Result<()> {
    record_recent(path);
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(path)
        .spawn()
        .map(|_| ())
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn format_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", size)
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("solyra-files-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn directories_sort_before_files_case_insensitively() {
        let root = temp_root("sort");
        fs::create_dir(root.join("zeta")).unwrap();
        fs::write(root.join("Alpha.txt"), "a").unwrap();
        fs::create_dir(root.join("beta")).unwrap();
        fs::write(root.join(".hidden"), "h").unwrap();
        let entries = read_entries(&root, false).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["beta", "zeta", "Alpha.txt"]);
        assert!(read_entries(&root, true).unwrap().iter().any(|e| e.hidden));
    }

    #[test]
    fn filtering_is_case_insensitive() {
        let root = temp_root("filter");
        fs::write(root.join("Notes.md"), "").unwrap();
        fs::write(root.join("image.png"), "").unwrap();
        let entries = read_entries(&root, false).unwrap();
        let got = filtered(&entries, "NOT");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Notes.md");
    }

    #[test]
    fn rename_rejects_paths_and_collisions() {
        let root = temp_root("rename");
        let src = root.join("one.txt");
        fs::write(&src, "").unwrap();
        fs::write(root.join("two.txt"), "").unwrap();
        assert!(rename_entry(&src, "bad/name").is_err());
        assert!(rename_entry(&src, "two.txt").is_err());
        let dest = rename_entry(&src, "renamed.txt").unwrap();
        assert!(dest.exists());
        assert!(!src.exists());
    }

    #[test]
    fn colour_tags_cycle_and_filter() {
        let root = temp_root("tags");
        let data = root.join("data");
        let file = root.join("tagged.txt");
        fs::write(&file, "").unwrap();
        let old_data = std::env::var_os("SOLYRA_FILES_DATA");
        unsafe { std::env::set_var("SOLYRA_FILES_DATA", &data) };

        assert_eq!(cycle_tag(&file).unwrap().as_deref(), Some("red"));
        assert_eq!(tag_for(&file).as_deref(), Some("red"));
        assert_eq!(paths_tagged("red"), vec![file.clone()]);
        assert!(paths_tagged("blue").is_empty());
        assert_eq!(cycle_tag(&file).unwrap().as_deref(), Some("orange"));
        for _ in 0..5 {
            let _ = cycle_tag(&file).unwrap();
        }
        assert_eq!(tag_for(&file), None);

        match old_data {
            Some(value) => unsafe { std::env::set_var("SOLYRA_FILES_DATA", value) },
            None => unsafe { std::env::remove_var("SOLYRA_FILES_DATA") },
        }
    }

    #[test]
    fn trash_round_trip_restores_with_trashinfo() {
        let root = temp_root("trash");
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let old_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &home) };

        let src = root.join("note one.txt");
        fs::write(&src, "hello").unwrap();
        let trashed = move_to_trash(&src).unwrap();
        assert!(!src.exists());
        assert!(trashed.exists());
        let info = home.join(".local/share/Trash/info").join(format!(
            "{}.trashinfo",
            trashed.file_name().unwrap().to_string_lossy()
        ));
        let text = fs::read_to_string(info).unwrap();
        assert!(text.contains("Path="));
        assert!(text.contains("note%20one.txt"));

        let restored = restore_from_trash(&trashed).unwrap();
        assert_eq!(restored, src);
        assert_eq!(fs::read_to_string(&restored).unwrap(), "hello");

        match old_home {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }

    #[test]
    fn sizes_format_readably() {
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(20 * 1024 * 1024), "20 MB");
    }
}

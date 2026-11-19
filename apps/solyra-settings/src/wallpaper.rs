//! Choosing your own background: validate, copy into Solyra's own folder, list.
//!
//! The image is copied so the wallpaper survives the original being moved or
//! deleted, and only PNG and JPEG are accepted (what the shell's `swaybg`
//! reads here).

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum Kind {
    Png,
    Jpeg,
}

pub fn sniff(head: &[u8]) -> Option<Kind> {
    if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(Kind::Png)
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(Kind::Jpeg)
    } else {
        None
    }
}

/// `~/.local/share/solyra/wallpapers`.
pub fn store_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/share"),
    };
    Some(base.join("solyra/wallpapers"))
}

/// A file name that is safe and readable: letters, digits, `-`, `_`, `.`.
pub fn clean_name(name: &str, kind: &Kind) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("wallpaper");
    let stem: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    let stem = if stem.trim_matches('-').is_empty() {
        "wallpaper".to_string()
    } else {
        stem
    };
    let ext = match kind {
        Kind::Png => "png",
        Kind::Jpeg => "jpg",
    };
    format!("{stem}.{ext}")
}

/// Copy `src` into `dest_dir` and return the new path. Never overwrites: a
/// name that exists gets `-2`, `-3`, ….
pub fn import(src: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let meta = fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", src.display()));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("{} is larger than 64 MiB", src.display()));
    }
    let mut head = [0u8; 12];
    let n = fs::File::open(src)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("{}: {e}", src.display()))?;
    let kind =
        sniff(&head[..n]).ok_or_else(|| format!("{} is not a PNG or JPEG image", src.display()))?;
    fs::create_dir_all(dest_dir).map_err(|e| format!("{}: {e}", dest_dir.display()))?;
    let name = clean_name(
        src.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        &kind,
    );
    let (stem, ext) = name.rsplit_once('.').expect("clean_name adds an extension");
    for i in 1..1000 {
        let candidate = if i == 1 {
            dest_dir.join(&name)
        } else {
            dest_dir.join(format!("{stem}-{i}.{ext}"))
        };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut out) => {
                let mut input = fs::File::open(src).map_err(|e| e.to_string())?;
                std::io::copy(&mut input, &mut out).map_err(|e| format!("copy failed: {e}"))?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", candidate.display())),
        }
    }
    Err("too many wallpapers with that name".into())
}

/// Imported wallpapers, newest first.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            matches!(
                e.path().extension().and_then(|x| x.to_str()),
                Some("png" | "jpg")
            )
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// Remove an imported wallpaper; refuses anything outside `dir`.
pub fn remove(dir: &Path, file: &Path) -> Result<(), String> {
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let file = file.canonicalize().map_err(|e| e.to_string())?;
    if file.parent() != Some(dir.as_path()) {
        return Err("not one of your imported wallpapers".into());
    }
    fs::remove_file(&file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solyra-wp-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0, 0, 0, 0, 0, 0, 0, 0];

    #[test]
    fn sniffing() {
        assert_eq!(sniff(&PNG), Some(Kind::Png));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some(Kind::Jpeg));
        assert_eq!(sniff(b"GIF89a"), None);
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(
            clean_name("My Photo (1).PNG", &Kind::Png),
            "My-Photo--1-.png"
        );
        assert_eq!(clean_name("../../etc/passwd", &Kind::Jpeg), "passwd.jpg");
        assert_eq!(clean_name("", &Kind::Png), "wallpaper.png");
        assert_eq!(clean_name("###", &Kind::Png), "wallpaper.png");
    }

    #[test]
    fn import_copies_and_never_overwrites() {
        let root = scratch("imp");
        let src = root.join("beach.png");
        fs::write(&src, PNG).unwrap();
        let dest = root.join("store");
        let a = import(&src, &dest).unwrap();
        let b = import(&src, &dest).unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with("beach.png") && b.ends_with("beach-2.png"));
        assert_eq!(fs::read(&a).unwrap(), PNG);
        assert!(src.exists(), "the original is left alone");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn import_rejects_non_images_and_directories() {
        let root = scratch("rej");
        let txt = root.join("a.png");
        fs::write(&txt, b"hello world, not an image").unwrap();
        assert!(
            import(&txt, &root.join("s"))
                .unwrap_err()
                .contains("not a PNG or JPEG")
        );
        assert!(
            import(&root, &root.join("s"))
                .unwrap_err()
                .contains("not a file")
        );
        assert!(import(&root.join("missing"), &root.join("s")).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_refuses_files_outside_the_store() {
        let root = scratch("rm");
        let store = root.join("store");
        fs::create_dir_all(&store).unwrap();
        let outside = root.join("keep.png");
        fs::write(&outside, PNG).unwrap();
        assert!(remove(&store, &outside).is_err());
        assert!(outside.exists());
        let inside = store.join("x.png");
        fs::write(&inside, PNG).unwrap();
        remove(&store, &inside).unwrap();
        assert!(!inside.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_finds_only_images() {
        let root = scratch("ls");
        fs::write(root.join("a.png"), PNG).unwrap();
        fs::write(root.join("b.txt"), b"x").unwrap();
        assert_eq!(list(&root).len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}

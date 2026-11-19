//! `key=value` config files that more than one program edits.
//!
//! The panel and the Settings app both write `~/.config/solyra/shell.conf`.
//! Each edits only the keys it owns, keeps everything else (including
//! comments), and writes atomically so a reader never sees half a file.

use std::path::{Path, PathBuf};

/// `~/.config/solyra/<name>` (honouring `XDG_CONFIG_HOME`).
pub fn config_path(name: &str) -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("solyra").join(name))
}

/// `existing` with each `(key, value)` replaced in place (or appended); a
/// `None` value removes the key. Unknown keys and comments are kept.
pub fn merge(existing: &str, pairs: &[(&str, Option<String>)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = vec![false; pairs.len()];
    for line in existing.lines() {
        let key = line.split_once('=').map(|(k, _)| k.trim());
        match key.and_then(|k| pairs.iter().position(|(pk, _)| *pk == k)) {
            Some(i) => {
                if let (Some(v), false) = (&pairs[i].1, done[i]) {
                    out.push(format!("{}={}", pairs[i].0, v));
                }
                done[i] = true;
            }
            None => out.push(line.to_string()),
        }
    }
    for (i, (k, v)) in pairs.iter().enumerate() {
        if let (Some(v), false) = (v, done[i]) {
            out.push(format!("{k}={v}"));
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// The value of `key`, if present (last one wins, like a re-read would).
pub fn get<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim())
        .next_back()
}

/// Merge `pairs` into the file at `path`, writing atomically.
pub fn update(path: &Path, pairs: &[(&str, Option<String>)]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let body = merge(&existing, pairs);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_foreign_keys_and_comments() {
        let out = merge(
            "# mine\nfoo=bar\ntheme=dark\nvolume=3\n",
            &[("theme", Some("light".into())), ("brightness", Some("80".into()))],
        );
        assert_eq!(out, "# mine\nfoo=bar\ntheme=light\nvolume=3\nbrightness=80\n");
    }

    #[test]
    fn none_removes_and_duplicates_collapse() {
        assert_eq!(merge("a=1\nx=/y\nb=2\n", &[("x", None)]), "a=1\nb=2\n");
        assert_eq!(merge("a=1\na=3\n", &[("a", Some("2".into()))]), "a=2\n");
    }

    #[test]
    fn get_reads_the_last_value() {
        assert_eq!(get("a=1\nb=2\na=3\n", "a"), Some("3"));
        assert_eq!(get("a=1\n", "z"), None);
    }

    #[test]
    fn update_writes_atomically_and_creates_directories() {
        let dir = std::env::temp_dir().join(format!("solyra-conf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested/shell.conf");
        update(&path, &[("theme", Some("light".into()))]).unwrap();
        update(&path, &[("volume", Some("4".into()))]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "theme=light\nvolume=4\n");
        assert!(!path.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

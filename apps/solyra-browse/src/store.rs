//! Bookmarks and history: JSON-lines files in `~/.local/share/solyra/browse`.
//! Plain files you can read, back up or delete by hand.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::address::html_escape;

pub const MAX_HISTORY: usize = 5000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub url: String,
    pub title: String,
}

pub fn data_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/share"),
    };
    Some(base.join("solyra/browse"))
}

fn is_web(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn read(path: &Path) -> Vec<Entry> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            Some(Entry {
                url: v.get("url")?.as_str()?.to_string(),
                title: v
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .filter(|e| is_web(&e.url))
        .collect()
}

fn write(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    for e in entries {
        writeln!(f, "{}", json!({"url": e.url, "title": e.title}))?;
    }
    f.sync_data()?;
    fs::rename(tmp, path)
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn bookmarks_path(&self) -> PathBuf {
        self.dir.join("bookmarks.jsonl")
    }

    fn history_path(&self) -> PathBuf {
        self.dir.join("history.jsonl")
    }

    pub fn bookmarks(&self) -> Vec<Entry> {
        read(&self.bookmarks_path())
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarks().iter().any(|b| b.url == url)
    }

    /// Add or remove; returns whether the page is bookmarked afterwards.
    pub fn toggle_bookmark(&self, url: &str, title: &str) -> std::io::Result<bool> {
        if !is_web(url) {
            return Ok(false);
        }
        let mut all = self.bookmarks();
        let now = if let Some(i) = all.iter().position(|b| b.url == url) {
            all.remove(i);
            false
        } else {
            all.push(Entry {
                url: url.into(),
                title: title.into(),
            });
            true
        };
        write(&self.bookmarks_path(), &all)?;
        Ok(now)
    }

    /// Newest first, one entry per URL.
    pub fn history(&self) -> Vec<Entry> {
        let mut seen = std::collections::HashSet::new();
        read(&self.history_path())
            .into_iter()
            .rev()
            .filter(|e| seen.insert(e.url.clone()))
            .collect()
    }

    /// Record a visit. Only web pages are kept, and the file is trimmed.
    pub fn visit(&self, url: &str, title: &str) -> std::io::Result<()> {
        if !is_web(url) {
            return Ok(());
        }
        let path = self.history_path();
        if let Some(d) = path.parent() {
            fs::create_dir_all(d)?;
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(f, "{}", json!({"url": url, "title": title}))?;
        drop(f);
        let all = read(&path);
        if all.len() > MAX_HISTORY + 500 {
            write(&path, &all[all.len() - MAX_HISTORY..])?;
        }
        Ok(())
    }

    pub fn clear_history(&self) -> std::io::Result<()> {
        match fs::remove_file(self.history_path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// The new-tab page. Everything from the web is escaped.
pub fn home_html(bookmarks: &[Entry], recent: &[Entry]) -> String {
    let list = |entries: &[Entry], empty: &str| -> String {
        if entries.is_empty() {
            return format!("<p class=empty>{empty}</p>");
        }
        let items: String = entries
            .iter()
            .map(|e| {
                let label = if e.title.is_empty() { &e.url } else { &e.title };
                format!(
                    "<li><a href=\"{}\">{}</a><span>{}</span></li>",
                    html_escape(&e.url),
                    html_escape(label),
                    html_escape(&e.url)
                )
            })
            .collect();
        format!("<ul>{items}</ul>")
    };
    format!(
        "<!doctype html><meta charset=utf-8><title>New tab</title><style>\
        body{{font:15px system-ui,sans-serif;background:#121016;color:#dcd6e2;margin:0;padding:48px 24px}}\
        main{{max-width:720px;margin:0 auto}}h1{{font-size:22px;margin:0 0 24px}}h2{{font-size:12px;letter-spacing:.08em;color:#a098aa;margin:32px 0 8px;text-transform:uppercase}}\
        ul{{list-style:none;padding:0;margin:0}}li{{display:flex;justify-content:space-between;gap:16px;padding:9px 0;border-bottom:1px solid #2a2430}}\
        a{{color:#dcd6e2;text-decoration:none}}a:hover{{color:#b48aad}}span{{color:#8b8394;font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:50%}}\
        .empty{{color:#8b8394}}</style><main><h1>Solyra Browse</h1><h2>Bookmarks</h2>{}<h2>Recent</h2>{}</main>",
        list(bookmarks, "No bookmarks yet. Use the star in the toolbar."),
        list(&recent[..recent.len().min(15)], "Nothing visited yet."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solyra-browse-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn bookmarks_toggle_and_persist() {
        let dir = scratch("bm");
        let s = Store::new(dir.clone());
        assert!(s.toggle_bookmark("https://a.example/", "A").unwrap());
        assert!(Store::new(dir.clone()).is_bookmarked("https://a.example/"));
        assert!(!s.toggle_bookmark("https://a.example/", "A").unwrap());
        assert!(s.bookmarks().is_empty());
        assert!(
            !s.toggle_bookmark("javascript:alert(1)", "x").unwrap(),
            "only web pages"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_is_newest_first_deduped_and_web_only() {
        let dir = scratch("hist");
        let s = Store::new(dir.clone());
        for (u, t) in [
            ("https://a/", "A"),
            ("https://b/", "B"),
            ("about:blank", "x"),
            ("https://a/", "A again"),
        ] {
            s.visit(u, t).unwrap();
        }
        let h = s.history();
        assert_eq!(
            h.iter().map(|e| e.url.as_str()).collect::<Vec<_>>(),
            ["https://a/", "https://b/"]
        );
        assert_eq!(h[0].title, "A again");
        s.clear_history().unwrap();
        assert!(s.history().is_empty());
        s.clear_history().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_is_trimmed() {
        let dir = scratch("trim");
        let s = Store::new(dir.clone());
        for i in 0..(MAX_HISTORY + 600) {
            s.visit(&format!("https://h/{i}"), "t").unwrap();
        }
        let n = read(&dir.join("history.jsonl")).len();
        assert!(n <= MAX_HISTORY + 500, "{n}");
        assert_eq!(
            s.history()[0].url,
            format!("https://h/{}", MAX_HISTORY + 599)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = scratch("bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("bookmarks.jsonl"),
            "garbage\n{\"url\":\"https://ok/\",\"title\":\"ok\"}\n{\"url\":\"file:///etc\"}\n",
        )
        .unwrap();
        let b = Store::new(dir.clone()).bookmarks();
        assert_eq!(b.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn home_page_escapes_what_the_web_provides() {
        let evil = Entry {
            url: "https://x/\"><script>alert(1)</script>".into(),
            title: "<img src=x onerror=alert(1)>".into(),
        };
        let html = home_html(std::slice::from_ref(&evil), std::slice::from_ref(&evil));
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;img"));
        assert!(home_html(&[], &[]).contains("No bookmarks yet"));
    }
}

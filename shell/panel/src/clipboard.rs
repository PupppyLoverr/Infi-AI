//! Droppy-style rich clipboard history for the notch / Dynamic Island.
//!
//! Automatically classifies copied items into Text (with character count),
//! Links (with domain/URL), Hex Colors (full swatch + RGB breakdown), and
//! Files/Media, supports Work / Personal category filters, pinning, favorites,
//! and JSON persistence at `$XDG_DATA_HOME/solyra/clipboard.json`.

use std::path::{Path, PathBuf};

pub const MAX_ITEMS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipCategory {
    All,
    Work,
    Personal,
}

impl ClipCategory {
    pub const TABS: [Self; 3] = [Self::All, Self::Work, Self::Personal];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Clipboard",
            Self::Work => "Work",
            Self::Personal => "Personal",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Work => "work",
            Self::Personal => "personal",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "work" => Self::Work,
            "personal" => Self::Personal,
            _ => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Link,
    Color(u8, u8, u8),
    Video,
}

impl ClipKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Link => "Link",
            Self::Color(_, _, _) => "Color",
            Self::Video => "Video",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipItem {
    pub id: u64,
    pub kind: ClipKind,
    pub text: String,
    pub age: String,
    pub category: ClipCategory,
    pub pinned: bool,
    pub favorite: bool,
}

impl ClipItem {
    pub fn footer(&self) -> String {
        match self.kind {
            ClipKind::Text => format!("{} characters", self.text.chars().count()),
            ClipKind::Link => self.text.clone(),
            ClipKind::Color(r, g, b) => format!("RGB {r}, {g}, {b}"),
            ClipKind::Video => self.text.clone(),
        }
    }
}

/// Parse `#RRGGBB` or `#RGB` hex color literals.
pub fn parse_hex_color(raw: &str) -> Option<(u8, u8, u8)> {
    let hex = raw.trim().strip_prefix('#')?;
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Classify copied text into Color, Link, Video/Media, or plain Text.
pub fn classify(raw: &str) -> ClipKind {
    let t = raw.trim();
    if let Some((r, g, b)) = parse_hex_color(t) {
        return ClipKind::Color(r, g, b);
    }
    if t.starts_with("https://") || t.starts_with("http://") {
        return ClipKind::Link;
    }
    let lower = t.to_ascii_lowercase();
    if lower.ends_with(".mp4")
        || lower.ends_with(".mov")
        || lower.ends_with(".mkv")
        || lower.ends_with(".webm")
    {
        return ClipKind::Video;
    }
    ClipKind::Text
}

fn store_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("solyra/clipboard.json"))
}

pub fn load() -> Vec<ClipItem> {
    load_from(store_path().as_deref())
}

fn load_from(path: Option<&Path>) -> Vec<ClipItem> {
    let Some(raw) = path.and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let text = v.get("text")?.as_str()?.to_string();
            let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(i as u64 + 1);
            let age = v
                .get("age")
                .and_then(|x| x.as_str())
                .unwrap_or("now")
                .to_string();
            let category = v
                .get("category")
                .and_then(|x| x.as_str())
                .map(ClipCategory::parse)
                .unwrap_or(ClipCategory::All);
            let pinned = v.get("pinned").and_then(|x| x.as_bool()).unwrap_or(false);
            let favorite = v.get("favorite").and_then(|x| x.as_bool()).unwrap_or(false);
            let kind = classify(&text);
            Some(ClipItem {
                id,
                kind,
                text,
                age,
                category,
                pinned,
                favorite,
            })
        })
        .take(MAX_ITEMS)
        .collect()
}

pub fn save(items: &[ClipItem]) {
    let Some(path) = store_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let arr: Vec<serde_json::Value> = items
        .iter()
        .take(MAX_ITEMS)
        .map(|it| {
            serde_json::json!({
                "id": it.id,
                "text": it.text,
                "age": it.age,
                "category": it.category.as_str(),
                "pinned": it.pinned,
                "favorite": it.favorite,
            })
        })
        .collect();
    if let Ok(body) = serde_json::to_string_pretty(&arr) {
        let _ = std::fs::write(path, body);
    }
}

pub fn push(items: &mut Vec<ClipItem>, text: String) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(pos) = items.iter().position(|it| it.text == text) {
        let existing = items.remove(pos);
        items.insert(0, existing);
        return;
    }
    let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
    let kind = classify(&text);
    items.insert(
        0,
        ClipItem {
            id: next_id,
            kind,
            text,
            age: "now".into(),
            category: ClipCategory::All,
            pinned: false,
            favorite: false,
        },
    );
    items.truncate(MAX_ITEMS);
}

pub fn copy_command(text: &str) -> String {
    let q = text.replace('\'', "'\\''");
    format!("printf '%s' '{q}' | wl-copy")
}

/// Curated fixture matching Droppy's rich clipboard cards (used in preview.rs).
#[cfg(test)]
pub fn demo_items() -> Vec<ClipItem> {
    vec![
        ClipItem {
            id: 1,
            kind: ClipKind::Text,
            text: "Launch review moved to 10:00.\nSame room.".into(),
            age: "14 min. ago".into(),
            category: ClipCategory::Work,
            pinned: true,
            favorite: false,
        },
        ClipItem {
            id: 2,
            kind: ClipKind::Link,
            text: "https://getdroppy.app/blog".into(),
            age: "6 min. ago".into(),
            category: ClipCategory::Personal,
            pinned: false,
            favorite: true,
        },
        ClipItem {
            id: 3,
            kind: ClipKind::Color(0xff, 0x9f, 0x43),
            text: "#FF9F43".into(),
            age: "now".into(),
            category: ClipCategory::Work,
            pinned: false,
            favorite: false,
        },
        ClipItem {
            id: 4,
            kind: ClipKind::Video,
            text: "showcase-clip.mp4".into(),
            age: "2 min. ago".into(),
            category: ClipCategory::Personal,
            pinned: false,
            favorite: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_colors_links_videos_and_text() {
        assert_eq!(classify("#FF9F43"), ClipKind::Color(255, 159, 67));
        assert_eq!(classify("#0af"), ClipKind::Color(0, 170, 255));
        assert_eq!(classify("https://getdroppy.app/blog"), ClipKind::Link);
        assert_eq!(classify("demo.mp4"), ClipKind::Video);
        assert_eq!(classify("Launch review moved to 10:00."), ClipKind::Text);
    }

    #[test]
    fn push_deduplicates_and_caps() {
        let mut items = Vec::new();
        push(&mut items, "hello".into());
        push(&mut items, "#FF9F43".into());
        push(&mut items, "hello".into());
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "hello");
        assert_eq!(items[1].footer(), "RGB 255, 159, 67");
    }
}

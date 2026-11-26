//! Synced LRC lyrics parser and lookup for Droppy's Notch Lyrics drawer and
//! Fullscreen Ambient Lyrics Lock Screen.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncedLyrics {
    pub lines: Vec<LyricLine>,
}

impl SyncedLyrics {
    /// Parse standard `[mm:ss.xx]` or `[mm:ss]` LRC text.
    pub fn parse_lrc(raw: &str) -> Self {
        let mut lines = Vec::new();
        for line in raw.lines() {
            let mut rest = line.trim();
            let mut stamps = Vec::new();
            while let Some(after_open) = rest.strip_prefix('[') {
                let Some(close) = after_open.find(']') else {
                    break;
                };
                let tag = &after_open[..close];
                if let Some(ms) = parse_timestamp(tag) {
                    stamps.push(ms);
                }
                rest = &after_open[close + 1..];
            }
            let text = rest.trim();
            if text.is_empty() || stamps.is_empty() {
                continue;
            }
            for time_ms in stamps {
                lines.push(LyricLine {
                    time_ms,
                    text: text.to_string(),
                });
            }
        }
        lines.sort_by_key(|l| l.time_ms);
        Self { lines }
    }

    /// Index of the line active at `pos_ms`.
    pub fn active_index(&self, pos_ms: u64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let mut idx = 0;
        for (i, line) in self.lines.iter().enumerate() {
            if line.time_ms <= pos_ms {
                idx = i;
            } else {
                break;
            }
        }
        Some(idx)
    }

    /// Return `(relative_offset, text)` around the active line (`0` = active).
    pub fn window(&self, pos_ms: u64, before: usize, after: usize) -> Vec<(isize, &str)> {
        let Some(active) = self.active_index(pos_ms) else {
            return Vec::new();
        };
        let start = active.saturating_sub(before);
        let end = (active + after + 1).min(self.lines.len());
        (start..end)
            .map(|i| (i as isize - active as isize, self.lines[i].text.as_str()))
            .collect()
    }
}

fn parse_timestamp(tag: &str) -> Option<u64> {
    let (mm, ss_part) = tag.split_once(':')?;
    let mins: u64 = mm.trim().parse().ok()?;
    let (secs, frac_ms) = if let Some((s, frac)) = ss_part.split_once('.') {
        let s: u64 = s.trim().parse().ok()?;
        let f = frac.trim();
        let ms: u64 = match f.len() {
            1 => f.parse::<u64>().ok()? * 100,
            2 => f.parse::<u64>().ok()? * 10,
            3.. => f[..3].parse::<u64>().ok()?,
            _ => 0,
        };
        (s, ms)
    } else {
        (ss_part.trim().parse().ok()?, 0)
    };
    Some(mins * 60_000 + secs * 1_000 + frac_ms)
}

/// Parse unsynced plain lyrics by evenly distributing lines across `duration_ms`.
pub fn parse_plain(raw: &str, duration_ms: u64) -> SyncedLyrics {
    let texts: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if texts.is_empty() {
        return SyncedLyrics::default();
    }
    let dur = duration_ms.max(60_000);
    let step = dur / (texts.len() as u64).max(1);
    let lines = texts
        .into_iter()
        .enumerate()
        .map(|(i, t)| LyricLine {
            time_ms: i as u64 * step,
            text: t.to_string(),
        })
        .collect();
    SyncedLyrics { lines }
}

fn slug(artist: &str, title: &str) -> String {
    let raw = format!("{artist}-{title}").to_ascii_lowercase();
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(96)
        .collect()
}

fn cache_path(artist: &str, title: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join(format!("solyra/lyrics/{}.lrc", slug(artist, title))))
}

/// Look up a local `.lrc` file in `$XDG_DATA_HOME/solyra/lyrics/` or the
/// LRCLIB cache in `$XDG_CACHE_HOME/solyra/lyrics/`.
pub fn load_local(artist: &str, title: &str) -> Option<SyncedLyrics> {
    let mut candidates = Vec::new();
    if let Some(base) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    {
        let dir = base.join("solyra/lyrics");
        candidates.push(dir.join(format!("{artist} - {title}.lrc")));
        candidates.push(dir.join(format!("{title}.lrc")));
    }
    if let Some(cp) = cache_path(artist, title) {
        candidates.push(cp);
    }
    for p in candidates {
        if let Ok(raw) = std::fs::read_to_string(&p) {
            let parsed = SyncedLyrics::parse_lrc(&raw);
            if !parsed.lines.is_empty() {
                return Some(parsed);
            }
        }
    }
    None
}

/// Shown when a track has no synced lyrics. Solyra never invents lyrics.
pub const NONE_MESSAGE: &str = "No synced lyrics for this track";

/// Original sample lines for previews and tests only (never shown for a
/// real track: live code uses [`load_local`] and [`NONE_MESSAGE`]).
#[cfg(test)]
pub fn sample() -> SyncedLyrics {
    SyncedLyrics::parse_lrc(
        "[00:00.00] Under a sky of ultramarine\n\
         [00:05.00] The water holds the moon\n\
         [00:12.00] Drop by drop, it finds its way to you\n\
         [00:20.00] Silver ripples drift into the blue\n\
         [00:27.00] Every light along the bay\n\
         [00:33.00] Leans in close and hums along\n",
    )
}

/// Parse an LRCLIB JSON response (`syncedLyrics` preferred, else `plainLyrics`).
pub fn parse_lrclib_json(raw: &str, duration_ms: u64) -> Option<SyncedLyrics> {
    let val: serde_json::Value = serde_json::from_str(raw).ok()?;
    if let Some(synced) = val.get("syncedLyrics").and_then(|v| v.as_str()) {
        let parsed = SyncedLyrics::parse_lrc(synced);
        if !parsed.lines.is_empty() {
            return Some(parsed);
        }
    }
    if let Some(plain) = val.get("plainLyrics").and_then(|v| v.as_str()) {
        let parsed = parse_plain(plain, duration_ms);
        if !parsed.lines.is_empty() {
            return Some(parsed);
        }
    }
    None
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Kick off a non-blocking LRCLIB lookup off the UI thread for any song playing
/// on any MPRIS/playerctl player, caching the synced `.lrc` locally for instant
/// reuse on subsequent frames and replays.
pub fn fetch_async(artist: String, title: String, duration_ms: u64) {
    if title.trim().is_empty() || load_local(&artist, &title).is_some() {
        return;
    }
    let Some(cache) = cache_path(&artist, &title) else {
        return;
    };
    let marker = cache.with_extension("pending");
    if marker.exists() {
        return;
    }
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&marker, "");
    std::thread::spawn(move || {
        let url = format!(
            "https://lrclib.net/api/get?artist_name={}&track_name={}",
            urlencode(&artist),
            urlencode(&title)
        );
        if let Ok(out) = std::process::Command::new("curl")
            .args([
                "-fsSL",
                "--max-time",
                "4",
                "-H",
                "User-Agent: SolyraOS/0.1 (https://github.com/PupppyLoverr/Infi-AI)",
                &url,
            ])
            .output()
        {
            if out.status.success() {
                let body = String::from_utf8_lossy(&out.stdout);
                if let Some(lyr) = parse_lrclib_json(&body, duration_ms) {
                    let mut lrc_out = String::new();
                    for l in &lyr.lines {
                        let m = l.time_ms / 60_000;
                        let s = (l.time_ms / 1_000) % 60;
                        let cs = (l.time_ms % 1_000) / 10;
                        lrc_out.push_str(&format!("[{m:02}:{s:02}.{cs:02}] {}\n", l.text));
                    }
                    let _ = std::fs::write(&cache, lrc_out);
                }
            }
        }
        let _ = std::fs::remove_file(&marker);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_and_tracks_active_line() {
        let lyr = sample();
        assert!(lyr.lines.len() >= 6);
        assert_eq!(lyr.active_index(6_000), Some(1));
        let win = lyr.window(6_000, 1, 3);
        assert_eq!(win[0], (-1, "Under a sky of ultramarine"));
        assert_eq!(win[1], (0, "The water holds the moon"));
        assert_eq!(win[2], (1, "Drop by drop, it finds its way to you"));
    }

    #[test]
    fn parses_lrclib_synced_and_plain_json() {
        let synced = r#"{"syncedLyrics":"[00:05.00] Line one\n[00:10.50] Line two"}"#;
        let lyr = parse_lrclib_json(synced, 120_000).expect("synced");
        assert_eq!(lyr.lines.len(), 2);
        assert_eq!(lyr.active_index(11_000), Some(1));

        let plain = r#"{"plainLyrics":"First verse\nSecond verse\nChorus"}"#;
        let p = parse_lrclib_json(plain, 90_000).expect("plain");
        assert_eq!(p.lines.len(), 3);
        assert_eq!(p.lines[1].time_ms, 30_000);
    }
}

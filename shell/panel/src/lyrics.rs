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

/// Look up a local `.lrc` file in `$XDG_DATA_HOME/solyra/lyrics/`.
pub fn load_local(artist: &str, title: &str) -> Option<SyncedLyrics> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let dir = base.join("solyra/lyrics");
    let candidates = [
        dir.join(format!("{artist} - {title}.lrc")),
        dir.join(format!("{title}.lrc")),
    ];
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
}

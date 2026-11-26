//! Terminal preferences: theme (window tint + ANSI palette), glass (how
//! clear the window is; seeded from the system-wide glass value in
//! ~/.config/solyra/look.json) and font size. Saved to
//! ~/.config/solyra/term.json and applied live to every pane.

use zgx::scene::Color;
use zgx::terminal::{Terminal, TerminalColors};

pub const THEMES: [&str; 3] = ["Glass", "Graphite", "Midnight"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    pub theme: usize,
    /// 0 = ultra clear .. 100 = fully tinted (the system glass scale).
    pub glass: u8,
    pub font: f32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { theme: 0, glass: system_glass().unwrap_or(45), font: 13.0 }
    }
}

fn config_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .map(|d| d.join("solyra"))
}

fn system_glass() -> Option<u8> {
    let text = std::fs::read_to_string(config_dir()?.join("look.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("glass")?.as_f64().map(|g| g.clamp(0., 100.) as u8)
}

impl Prefs {
    pub fn load() -> Self {
        let mut p = Self::default();
        if let Some(v) = config_dir()
            .and_then(|d| std::fs::read_to_string(d.join("term.json")).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            if let Some(t) = v.get("theme").and_then(|x| x.as_u64()) {
                p.theme = (t as usize).min(THEMES.len() - 1);
            }
            if let Some(g) = v.get("glass").and_then(|x| x.as_u64()) {
                p.glass = g.min(100) as u8;
            }
            if let Some(f) = v.get("font").and_then(|x| x.as_f64()) {
                p.font = (f as f32).clamp(9., 24.);
            }
        }
        p
    }

    pub fn save(&self) {
        if let Some(d) = config_dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(
                d.join("term.json"),
                serde_json::json!({ "theme": self.theme, "glass": self.glass, "font": self.font }).to_string(),
            );
        }
    }

    /// The window's single glass tint.
    pub fn tint(&self) -> Color {
        let (r, g, b) = match self.theme {
            1 => (24, 24, 28),
            2 => (12, 16, 32),
            _ => (10, 28, 27),
        };
        // 0 -> alpha 70 (ultra clear), 100 -> 245 (fully tinted).
        Color(r, g, b, (70.0 + self.glass as f32 * 1.75) as u8)
    }

    pub fn colors(&self) -> TerminalColors {
        let pal: [u32; 16] = match self.theme {
            1 => [0x1c1c1e, 0xff6b6b, 0x8fd694, 0xe8c66b, 0x8cb4ff, 0xc9a2ff, 0x7fd6d0, 0xd8d8dc,
                  0x6e6e73, 0xff8f8f, 0xa9e5ad, 0xf2d891, 0xadc9ff, 0xdcc1ff, 0xa1e6e1, 0xffffff],
            2 => [0x0d1020, 0xff7a90, 0x7ee0b0, 0xf0d080, 0x7aa8ff, 0xb59cff, 0x6fd5f0, 0xd5dcf0,
                  0x5c6580, 0xff9aab, 0x9ff0c8, 0xf8e0a0, 0x9cc0ff, 0xccbaff, 0x9ae4f8, 0xffffff],
            _ => [0x1d2021, 0xff6b6b, 0x7ee787, 0xf2cc60, 0x79b8ff, 0xd2a8ff, 0x76e3ea, 0xe6edf3,
                  0x6e7681, 0xffa198, 0xa5f3a0, 0xf8e3a1, 0xa5d6ff, 0xe2c5ff, 0xb3f0ff, 0xffffff],
        };
        let c = |x: u32| Some(Color((x >> 16) as u8, (x >> 8) as u8, x as u8, 255));
        let mut ansi = [None; 16];
        for (i, v) in pal.iter().enumerate() {
            ansi[i] = c(*v);
        }
        TerminalColors {
            foreground: c(0xe8ece9),
            background: Some(Color(0, 0, 0, 0)),
            cursor: c(0xf2f2f2),
            cursor_text: c(0x101614),
            selection_background: Some(Color(255, 255, 255, 56)),
            selection_foreground: None,
            ansi,
        }
    }

    /// Apply colours and font size to one terminal.
    pub fn apply(&self, term: &Terminal) {
        term.set_colors(self.colors());
        let mut o = term.options();
        if o.font_size != Some(self.font) {
            o.font_size = Some(self.font);
            term.set_options(o);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glass_scale_maps_clear_to_tinted() {
        let clear = Prefs { theme: 0, glass: 0, font: 13. }.tint();
        let tinted = Prefs { theme: 0, glass: 100, font: 13. }.tint();
        assert!(clear.3 < 80 && tinted.3 > 240);
        assert_eq!(Prefs { theme: 1, glass: 50, font: 13. }.colors().ansi.iter().filter(|c| c.is_some()).count(), 16);
    }
}

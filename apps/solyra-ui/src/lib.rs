//! The Solyra look for zgx apps. Colours come from `solyra-tokens`, the same
//! source the shell panel paints from, so an app and the shell cannot drift.

use solyra_tokens::{Rgba, Theme, ThemeKind};
use zgx::scene::Color;

/// A token colour as zgx's `Color`.
pub fn color(c: Rgba) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

/// The theme the user chose in `~/.config/solyra/shell.conf` (`theme=dark|light`),
/// dark when the file or key is absent.
pub fn current_theme() -> Theme {
    Theme::for_kind(theme_kind_from(&config_text()))
}

pub fn theme_kind_from(conf: &str) -> ThemeKind {
    conf.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "theme")
        .and_then(|(_, v)| ThemeKind::parse(v))
        .unwrap_or(ThemeKind::Dark)
}

fn config_text() -> String {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    base.and_then(|b| std::fs::read_to_string(b.join("solyra/shell.conf")).ok())
        .unwrap_or_default()
}

/// Opaque window background for a theme (apps are not translucent glass).
pub fn window_bg(theme: &Theme) -> Color {
    match theme.surface {
        Rgba(r, g, b, _) => Color(r, g, b, 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_defaults_to_dark_and_parses_light() {
        assert_eq!(theme_kind_from(""), ThemeKind::Dark);
        assert_eq!(theme_kind_from("volume=3\ntheme=light\n"), ThemeKind::Light);
        assert_eq!(theme_kind_from("theme=neon"), ThemeKind::Dark);
    }

    #[test]
    fn token_colour_converts_exactly() {
        assert_eq!(color(Rgba(1, 2, 3, 4)), Color(1, 2, 3, 4));
    }
}

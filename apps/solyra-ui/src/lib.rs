//! The Solyra look for zgx apps. Colours come from `solyra-tokens`, the same
//! source the shell panel paints from, so an app and the shell cannot drift.
//!
//! **Frosted glass.** With `app_glass` on (the default) and "Solid surfaces"
//! off, apps open transparent windows and paint translucent backgrounds; the
//! compositor blurs what is behind (the window rule in `solyra-config.kdl`).
//! The sidebar is the most see-through layer, the content area denser so
//! text always reads, as on macOS. With glass off, the same colours are
//! opaque.
//!
//! [`scope`] maps zgx's own theme tokens (used by every `zgx-components`
//! control) onto Solyra's, so segmented controls, tabs and checkboxes look
//! like the shell, accent included.

pub mod chrome;
pub mod icons;

use solyra_conf::look::Look;
use solyra_tokens::{Accent, Rgba, Theme, ThemeKind};
use zgx::compose::prelude::*;
use zgx::reactive::Signal;
use zgx::scene::Color;
use zgx::theme::{tok, ThemeOverride};

/// A token colour as zgx's `Color`.
pub fn color(c: Rgba) -> Color {
    Color(c.0, c.1, c.2, c.3)
}

fn with_alpha(c: Rgba, a: u8) -> Color {
    Color(c.0, c.1, c.2, a)
}

/// What an app needs to know to look like Solyra.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub theme: Theme,
    pub kind: ThemeKind,
    /// Translucent window over the compositor's blur.
    pub glass: bool,
}

impl Style {
    pub fn from_conf(conf: &str) -> Self {
        let kind = theme_kind_from(conf);
        let look = Look::from_conf(conf);
        let accent = Accent::parse(&look.accent).unwrap_or(Accent::Blue);
        let glass = !look.opaque && flag(conf, "app_glass", true);
        Self {
            theme: Theme::with(kind, accent),
            kind,
            glass,
        }
    }

    pub fn dark(&self) -> bool {
        self.kind == ThemeKind::Dark
    }

    /// The content area: dense enough that body text reads over anything.
    pub fn content_bg(&self) -> Color {
        with_alpha(self.theme.surface, if self.glass { 214 } else { 255 })
    }

    /// The sidebar and the tab strip: the most see-through layer.
    pub fn sidebar_bg(&self) -> Color {
        let base = if self.dark() {
            Rgba(0x12, 0x10, 0x17, 255)
        } else {
            Rgba(0xec, 0xe8, 0xf0, 255)
        };
        with_alpha(base, if self.glass { 150 } else { 255 })
    }

    /// Cards and grouped rows inside the content.
    pub fn card_bg(&self) -> Color {
        if self.dark() {
            Color(255, 255, 255, 12)
        } else {
            Color(255, 255, 255, 170)
        }
    }

    pub fn hairline(&self) -> Color {
        color(self.theme.hairline)
    }

    /// zgx's tokens in Solyra's colours.
    pub fn overrides(&self) -> ThemeOverride {
        let t = &self.theme;
        let dark = self.dark();
        let wash = |a: u8| {
            if dark {
                Color(255, 255, 255, a)
            } else {
                Color(0, 0, 0, a)
            }
        };
        ThemeOverride::new()
            .with(tok::BACKGROUND, self.content_bg())
            .with(tok::SURFACE_SUNKEN, color(t.field))
            .with(tok::SURFACE_BASE, self.card_bg())
            .with(
                tok::SURFACE_RAISED,
                if dark {
                    Color(255, 255, 255, 30)
                } else {
                    Color(255, 255, 255, 255)
                },
            )
            .with(tok::SURFACE_OVERLAY, with_alpha(t.surface, 245))
            .with(tok::TEXT_PRIMARY, color(t.text))
            .with(tok::TEXT_SECONDARY, color(t.text_med))
            .with(tok::TEXT_TERTIARY, color(t.text_dim))
            .with(tok::BORDER, color(t.hairline))
            .with(tok::BORDER_SUBTLE, color(t.separator))
            .with(tok::BORDER_STRONG, color(t.selected_border))
            .with(tok::ACCENT, color(t.accent))
            .with(tok::ACCENT_HOVER, color(t.selected_border))
            .with(tok::ACCENT_TEXT, color(t.on_accent))
            .with(tok::DANGER, color(t.danger))
            .with(tok::WARNING, color(t.warning))
            .with(tok::SUCCESS, color(t.success))
            .with(tok::SELECTION_ITEM, color(t.selected_fill))
            .with(tok::FOCUS_RING, color(t.accent))
            .with(tok::HOVER, wash(14))
            .with(tok::PRESSED, wash(26))
            .with(tok::GLASS_TINT, self.sidebar_bg())
    }
}

/// The look the person chose in `~/.config/solyra/shell.conf`.
pub fn current() -> Style {
    Style::from_conf(&config_text())
}

/// The theme alone (kept for callers that only need colours).
pub fn current_theme() -> Theme {
    current().theme
}

pub fn theme_kind_from(conf: &str) -> ThemeKind {
    solyra_conf::get(conf, "theme")
        .and_then(ThemeKind::parse)
        .unwrap_or(ThemeKind::Dark)
}

fn flag(conf: &str, key: &str, default: bool) -> bool {
    match solyra_conf::get(conf, key).map(|v| v.to_ascii_lowercase()) {
        Some(v) if matches!(v.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(v) if matches!(v.as_str(), "0" | "false" | "no" | "off") => false,
        _ => default,
    }
}

fn config_text() -> String {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    base.and_then(|b| std::fs::read_to_string(b.join("solyra/shell.conf")).ok())
        .unwrap_or_default()
}

/// Opaque window background for a theme.
pub fn window_bg(theme: &Theme) -> Color {
    with_alpha(theme.surface, 255)
}

/// Wrap an app's root so zgx controls inside use Solyra's colours.
pub fn scope(style: &Style, child: View) -> View {
    theme_scope(style.overrides(), child)
}

/// A sidebar entry: accent-tinted pill when it is the selected page.
pub fn sidebar_item(style: &Style, label: &str, index: usize, selected: Signal<usize>) -> View {
    let (sel_style, sel_a11y, click) = (selected.clone(), selected.clone(), selected);
    let t = style.theme;
    button()
        .h(32.)
        .px(12.)
        .rounded(8.)
        .border(0.)
        .items_center()
        .selected_when(move || sel_a11y.get() == index)
        .reactive_style(move || {
            if sel_style.get() == index {
                Styles::new()
                    .bg(color(t.selected_fill))
                    .text_color(color(t.text))
            } else {
                Styles::new()
                    .bg(Color(0, 0, 0, 0))
                    .text_color(color(t.text_med))
            }
        })
        .hover(|s| s.text_color(color(t.text)))
        .on_click(move || {
            click.set(index);
        })
        .child(text(label.to_string()))
}

/// A titled group of rows on a card, as System Settings groups them.
pub fn group(style: &Style, title: &str, body: View) -> View {
    column()
        .gap(8.)
        .child(
            text(title.to_string())
                .text_size(12.)
                .font_bold()
                .text_color(color(style.theme.text_dim)),
        )
        .child(
            body.p(14.)
                .rounded(12.)
                .bg(style.card_bg())
                .border(1.)
                .border_color(style.hairline()),
        )
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

    #[test]
    fn glass_by_default_and_solid_surfaces_turns_it_off() {
        let s = Style::from_conf("");
        assert!(s.glass);
        assert!(s.content_bg().3 < 255 && s.sidebar_bg().3 < s.content_bg().3);
        let o = Style::from_conf("opaque=1\n");
        assert!(!o.glass);
        assert_eq!(o.content_bg().3, 255);
        assert_eq!(o.sidebar_bg().3, 255);
        assert!(!Style::from_conf("app_glass=0\n").glass);
    }

    #[test]
    fn the_accent_follows_the_setting() {
        let mauve = Style::from_conf("").theme.accent;
        let teal = Style::from_conf("accent=teal\n").theme.accent;
        assert_ne!(mauve, teal);
    }
}

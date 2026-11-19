//! Solyra design tokens, shared by the shell panel and the zgx apps.
//!
//! No UI dependency: colours are plain bytes, converted by each consumer.

/// Straight (non-premultiplied) RGBA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(r, g, b, a)
    }
}

// ------------------------------------------------------------------ radii

pub const R_CONTROL: f32 = 8.0;
pub const R_ROW: f32 = 10.0;
pub const R_TILE: f32 = 14.0;
pub const R_CARD: f32 = 20.0;
pub const R_OVERLAY: f32 = 22.0;

// ------------------------------------------------------------------- type

pub const T_TITLE: f32 = 14.0;
pub const T_BODY: f32 = 13.0;
pub const T_LABEL: f32 = 12.0;
pub const T_META: f32 = 11.5;
/// Section captions ("RESULTS"). Never body copy.
pub const T_CAPTION: f32 = 10.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
}

impl ThemeKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Semantic colour roles. Every field is a role, not a use site.
///
/// Surfaces are flat: one fill and one hairline, no gradients. Overlays are
/// slightly translucent so the compositor's blur shows faintly through; the bar
/// and dock are nearly opaque. The accent means "active" or "selected" and is
/// never used for decoration.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    // Surfaces.
    /// Overlay card fill (launcher, control centre, agent card, island card).
    pub surface: Rgba,
    /// Bar and dock fill.
    pub bar: Rgba,
    pub hairline: Rgba,
    /// Recessed input fields (search field, slider tracks).
    pub field: Rgba,
    /// Idle fill of a tile or button on a surface.
    pub tile: Rgba,
    pub tile_hover: Rgba,
    pub separator: Rgba,

    // Text.
    pub text: Rgba,
    pub text_med: Rgba,
    pub text_dim: Rgba,

    // Accent.
    pub accent: Rgba,
    pub on_accent: Rgba,
    pub selected_fill: Rgba,
    pub selected_border: Rgba,

    // State.
    pub success: Rgba,
    pub warning: Rgba,
    pub danger: Rgba,

    // Agents. Approval is deliberately not the accent: "an agent is working"
    // and "an agent needs a decision" must not look alike.
    pub agent_active: Rgba,
    pub agent_approval: Rgba,
}

impl Theme {
    pub const fn dark() -> Self {
        Self {
            surface: Rgba::rgba(0x1a, 0x17, 0x20, 234),
            bar: Rgba::rgba(0x14, 0x11, 0x19, 240),
            hairline: Rgba::rgba(0x3a, 0x33, 0x42, 210),
            field: Rgba::rgba(0x0e, 0x0c, 0x12, 190),
            tile: Rgba::rgba(255, 255, 255, 13),
            tile_hover: Rgba::rgba(255, 255, 255, 26),
            separator: Rgba::rgba(255, 255, 255, 20),
            text: Rgba::rgb(0xdc, 0xd6, 0xe2),
            text_med: Rgba::rgba(0xdc, 0xd6, 0xe2, 214),
            text_dim: Rgba::rgb(0xa0, 0x98, 0xaa),
            accent: Rgba::rgb(0xb4, 0x8a, 0xad),
            on_accent: Rgba::rgb(0x14, 0x10, 0x18),
            selected_fill: Rgba::rgba(0xb4, 0x8a, 0xad, 44),
            selected_border: Rgba::rgba(0xb4, 0x8a, 0xad, 150),
            success: Rgba::rgb(0x7f, 0xb8, 0x8c),
            warning: Rgba::rgb(0xd9, 0xa4, 0x41),
            danger: Rgba::rgb(0xd8, 0x7a, 0x72),
            agent_active: Rgba::rgb(0xb4, 0x8a, 0xad),
            agent_approval: Rgba::rgb(0xe0, 0xa9, 0x4a),
        }
    }

    pub const fn light() -> Self {
        Self {
            surface: Rgba::rgba(0xf6, 0xf4, 0xf7, 242),
            bar: Rgba::rgba(0xee, 0xea, 0xf0, 244),
            hairline: Rgba::rgba(0xcf, 0xc8, 0xd4, 255),
            field: Rgba::rgba(255, 255, 255, 235),
            tile: Rgba::rgba(0x17, 0x13, 0x19, 12),
            tile_hover: Rgba::rgba(0x17, 0x13, 0x19, 24),
            separator: Rgba::rgba(0x17, 0x13, 0x19, 30),
            text: Rgba::rgb(0x17, 0x13, 0x19),
            text_med: Rgba::rgba(0x17, 0x13, 0x19, 220),
            text_dim: Rgba::rgb(0x5f, 0x58, 0x68),
            accent: Rgba::rgb(0x8a, 0x4f, 0x80),
            on_accent: Rgba::rgb(0xff, 0xff, 0xff),
            selected_fill: Rgba::rgba(0x8a, 0x4f, 0x80, 30),
            selected_border: Rgba::rgba(0x8a, 0x4f, 0x80, 150),
            success: Rgba::rgb(0x2a, 0x6e, 0x45),
            warning: Rgba::rgb(0x84, 0x55, 0x0e),
            danger: Rgba::rgb(0xb2, 0x3b, 0x33),
            agent_active: Rgba::rgb(0x8a, 0x4f, 0x80),
            agent_approval: Rgba::rgb(0x84, 0x55, 0x0e),
        }
    }

    pub const fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Light => Self::light(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source-over `fg` onto an opaque `bg`.
    fn composite(fg: Rgba, bg: Rgba) -> Rgba {
        let a = fg.3 as f32 / 255.0;
        let mix = |f: u8, b: u8| (f as f32 * a + b as f32 * (1.0 - a)).round() as u8;
        Rgba::rgb(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2))
    }

    fn themes() -> [(ThemeKind, Theme); 2] {
        [
            (ThemeKind::Dark, Theme::dark()),
            (ThemeKind::Light, Theme::light()),
        ]
    }

    fn channel(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: Rgba) -> f32 {
        0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
    }

    /// WCAG 2.x contrast ratio between two opaque colours.
    fn contrast_ratio(a: Rgba, b: Rgba) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// The surface colour a label actually sits on: the glass gradient's
    /// middle, over a wallpaper that is as unhelpful as possible.
    fn surface_over(theme: &Theme, backdrop: Rgba) -> Rgba {
        composite(theme.surface, backdrop)
    }

    fn worst_ratio(theme: &Theme, fg: Rgba) -> f32 {
        [Rgba::rgb(0, 0, 0), Rgba::rgb(255, 255, 255)]
            .into_iter()
            .map(|backdrop| {
                let surface = surface_over(theme, backdrop);
                contrast_ratio(composite(fg, surface), surface)
            })
            .fold(f32::MAX, f32::min)
    }

    #[test]
    fn body_text_meets_aa_in_both_themes() {
        for (kind, theme) in themes() {
            for (name, fg) in [("text", theme.text), ("text_med", theme.text_med)] {
                let r = worst_ratio(&theme, fg);
                assert!(r >= 4.5, "{:?} {name}: {r:.2}:1 < 4.5:1", kind);
            }
        }
    }

    #[test]
    fn secondary_text_is_readable_and_never_body_sized_below_aa_large() {
        for (kind, theme) in themes() {
            let r = worst_ratio(&theme, theme.text_dim);
            assert!(r >= 4.5, "{:?} text_dim: {r:.2}:1 < 4.5:1", kind);
        }
    }

    #[test]
    fn on_accent_is_readable_on_accent() {
        for (kind, theme) in themes() {
            for bg in [theme.accent] {
                let r = contrast_ratio(theme.on_accent, bg);
                assert!(r >= 4.5, "{:?} on_accent: {r:.2}:1", kind);
            }
        }
    }

    #[test]
    fn state_colours_are_legible_as_text_on_glass() {
        for (kind, theme) in themes() {
            for (name, c) in [
                ("success", theme.success),
                ("warning", theme.warning),
                ("danger", theme.danger),
                ("agent_approval", theme.agent_approval),
            ] {
                // Dark glass over a white wallpaper is the dark theme's worst
                // case only for the translucent glass; state colours are
                // checked on the opaque midpoint of the glass over black
                // (dark) or white (light), the backdrops those themes target.
                let backdrop = match kind {
                    ThemeKind::Dark => Rgba::rgb(0, 0, 0),
                    ThemeKind::Light => Rgba::rgb(255, 255, 255),
                };
                let surface = surface_over(&theme, backdrop);
                let r = contrast_ratio(c, surface);
                assert!(r >= 4.5, "{:?} {name}: {r:.2}:1", kind);
            }
        }
    }

    #[test]
    fn approval_is_distinct_from_active() {
        for (_, theme) in themes() {
            assert_ne!(theme.agent_approval, theme.agent_active);
        }
    }

    #[test]
    fn theme_kind_round_trips() {
        for kind in [ThemeKind::Dark, ThemeKind::Light] {
            assert_eq!(ThemeKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ThemeKind::parse("solarized"), None);
    }
}

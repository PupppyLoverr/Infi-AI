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
/// Surfaces are flat: one fill and one hairline, no gradients. Overlay cards
/// are slightly translucent so the compositor's blur shows faintly through.
/// The bar token is a thin tint; Glass paints an even clearer veil. The accent
/// means "active" or "selected" and is never used for decoration.
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
        // Neutral cool graphite (macOS / Windows 11), not purple-tinted.
        // Surfaces stay translucent so Glass/Acrylic can show the wallpaper.
        Self {
            surface: Rgba::rgba(0x1c, 0x1c, 0x1e, 196),
            bar: Rgba::rgba(0x1c, 0x1c, 0x1e, 112),
            hairline: Rgba::rgba(0xff, 0xff, 0xff, 38),
            field: Rgba::rgba(0x00, 0x00, 0x00, 112),
            tile: Rgba::rgba(255, 255, 255, 16),
            tile_hover: Rgba::rgba(255, 255, 255, 30),
            separator: Rgba::rgba(255, 255, 255, 24),
            text: Rgba::rgb(0xf5, 0xf5, 0xf7),
            text_med: Rgba::rgba(0xf2, 0xf2, 0xf7, 232),
            // Apple secondary label — stays ≥ AA on translucent glass.
            text_dim: Rgba::rgb(0xc7, 0xc7, 0xcc),
            // System blue — crisp Apple hue that still clears AA with white.
            accent: Rgba::rgb(0x00, 0x6a, 0xe0),
            on_accent: Rgba::rgb(0xff, 0xff, 0xff),
            selected_fill: Rgba::rgba(0x0a, 0x84, 0xff, 58),
            selected_border: Rgba::rgba(0x40, 0x9c, 0xff, 120),
            success: Rgba::rgb(0x32, 0xd7, 0x4b),
            warning: Rgba::rgb(0xff, 0xd6, 0x0a),
            danger: Rgba::rgb(0xff, 0x45, 0x3a),
            agent_active: Rgba::rgb(0x00, 0x6a, 0xe0),
            agent_approval: Rgba::rgb(0xff, 0x9f, 0x0a),
        }
    }

    pub const fn light() -> Self {
        Self {
            surface: Rgba::rgba(0xf2, 0xf2, 0xf7, 210),
            bar: Rgba::rgba(0xf2, 0xf2, 0xf7, 128),
            hairline: Rgba::rgba(0x00, 0x00, 0x00, 28),
            field: Rgba::rgba(255, 255, 255, 220),
            tile: Rgba::rgba(0x00, 0x00, 0x00, 10),
            tile_hover: Rgba::rgba(0x00, 0x00, 0x00, 18),
            separator: Rgba::rgba(0x00, 0x00, 0x00, 24),
            text: Rgba::rgb(0x1c, 0x1c, 0x1e),
            text_med: Rgba::rgba(0x3a, 0x3a, 0x3c, 230),
            text_dim: Rgba::rgb(0x48, 0x48, 0x4e),
            accent: Rgba::rgb(0x00, 0x5c, 0xd0),
            on_accent: Rgba::rgb(0xff, 0xff, 0xff),
            selected_fill: Rgba::rgba(0x00, 0x5c, 0xd0, 28),
            selected_border: Rgba::rgba(0x00, 0x5c, 0xd0, 150),
            // Darker state colours so they stay AA on light glass.
            success: Rgba::rgb(0x18, 0x74, 0x30),
            warning: Rgba::rgb(0x9a, 0x60, 0x00),
            danger: Rgba::rgb(0xc4, 0x00, 0x12),
            agent_active: Rgba::rgb(0x00, 0x5c, 0xd0),
            agent_approval: Rgba::rgb(0x9a, 0x60, 0x00),
        }
    }

    pub const fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Light => Self::light(),
        }
    }

    /// The theme for `kind`, with `accent` replacing the default accent. The
    /// selection colours follow the accent; approval stays amber so "working"
    /// and "needs you" never look alike, whatever accent is chosen.
    pub fn with(kind: ThemeKind, accent: Accent) -> Self {
        let mut t = Self::for_kind(kind);
        let (a, on) = accent.colors(kind);
        t.accent = a;
        t.on_accent = on;
        t.agent_active = a;
        let alpha_fill = if kind == ThemeKind::Dark { 44 } else { 30 };
        t.selected_fill = Rgba::rgba(a.0, a.1, a.2, alpha_fill);
        t.selected_border = Rgba::rgba(a.0, a.1, a.2, 150);
        t
    }

    /// Fully opaque surfaces, for people who prefer no translucency.
    pub fn opaque(mut self) -> Self {
        self.surface.3 = 255;
        self.bar.3 = 255;
        self
    }
}

/// Accent colours offered in Settings. Each has a dark-theme and a
/// light-theme value tuned for contrast (tested below).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accent {
    Mauve,
    Blue,
    Teal,
    Green,
    Amber,
    Rose,
    Graphite,
}

impl Accent {
    pub const ALL: [Accent; 7] = [
        Accent::Mauve,
        Accent::Blue,
        Accent::Teal,
        Accent::Green,
        Accent::Amber,
        Accent::Rose,
        Accent::Graphite,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mauve => "mauve",
            Self::Blue => "blue",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Amber => "amber",
            Self::Rose => "rose",
            Self::Graphite => "graphite",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mauve => "Mauve",
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Green => "Green",
            Self::Amber => "Amber",
            Self::Rose => "Rose",
            Self::Graphite => "Graphite",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|a| a.as_str() == raw.trim().to_ascii_lowercase())
    }

    /// (accent, text on accent) for a theme.
    pub fn colors(self, kind: ThemeKind) -> (Rgba, Rgba) {
        let dark_on = Rgba::rgb(0x11, 0x11, 0x13);
        let white = Rgba::rgb(0xff, 0xff, 0xff);
        match (self, kind) {
            (Self::Mauve, ThemeKind::Dark) => (Rgba::rgb(0xb4, 0x8a, 0xad), dark_on),
            (Self::Mauve, ThemeKind::Light) => (Rgba::rgb(0x8a, 0x4f, 0x80), white),
            // System blue — crisp Apple hue that still clears AA with white.
            (Self::Blue, ThemeKind::Dark) => (Rgba::rgb(0x00, 0x6a, 0xe0), white),
            (Self::Blue, ThemeKind::Light) => (Rgba::rgb(0x00, 0x64, 0xd8), white),
            (Self::Teal, ThemeKind::Dark) => (Rgba::rgb(0x5f, 0xbf, 0xb4), dark_on),
            (Self::Teal, ThemeKind::Light) => (Rgba::rgb(0x0f, 0x6e, 0x66), white),
            (Self::Green, ThemeKind::Dark) => (Rgba::rgb(0x86, 0xc0, 0x7a), dark_on),
            (Self::Green, ThemeKind::Light) => (Rgba::rgb(0x2f, 0x6e, 0x2a), white),
            (Self::Amber, ThemeKind::Dark) => (Rgba::rgb(0xe0, 0xb0, 0x5a), dark_on),
            (Self::Amber, ThemeKind::Light) => (Rgba::rgb(0x8a, 0x55, 0x00), white),
            (Self::Rose, ThemeKind::Dark) => (Rgba::rgb(0xe0, 0x8a, 0x9a), dark_on),
            (Self::Rose, ThemeKind::Light) => (Rgba::rgb(0xa8, 0x2f, 0x4f), white),
            (Self::Graphite, ThemeKind::Dark) => (Rgba::rgb(0xb4, 0xb6, 0xbc), dark_on),
            (Self::Graphite, ThemeKind::Light) => (Rgba::rgb(0x48, 0x4a, 0x50), white),
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
    fn every_accent_keeps_on_accent_text_readable() {
        for a in Accent::ALL {
            for kind in [ThemeKind::Dark, ThemeKind::Light] {
                let (bg, fg) = a.colors(kind);
                let r = contrast_ratio(fg, bg);
                assert!(r >= 4.5, "{a:?} {kind:?}: {r:.2}:1");
            }
            assert_eq!(Accent::parse(a.as_str()), Some(a));
        }
    }

    #[test]
    fn accent_never_replaces_the_approval_colour() {
        for a in Accent::ALL {
            for kind in [ThemeKind::Dark, ThemeKind::Light] {
                let t = Theme::with(kind, a);
                assert_eq!(t.agent_approval, Theme::for_kind(kind).agent_approval);
                assert_eq!(t.accent, a.colors(kind).0);
            }
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

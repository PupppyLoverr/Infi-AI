//! Agent-workspace desk card: clock, project placeholder, and environment chips.
//!
//! Shown when `desktop_mode=agent`. The panel keeps a real layer-shell surface
//! on the right of the desktop (`solyra-desk`). The same card is also painted
//! into the agents overlay when that surface is open and empty.

use crate::canvas::{text_width, Canvas, Fonts, Rgba};
use crate::tokens::Theme;

pub const W: i32 = 320;
pub const H: i32 = 280;

/// Three lines until a project folder is pinned.
pub const PROJECT_PLACEHOLDER: [&str; 3] = [
    "No project open",
    "Pin a folder to keep",
    "agent context here",
];

pub struct Glance<'a> {
    pub clock: &'a str,
    pub date: &'a str,
    pub project_lines: &'a [&'a str],
    /// 0 = Cloud, 1 = Local, 2 = Worktree.
    pub environment: usize,
}

/// Paint the desk card into `cv` at the given origin. Returns the card size.
pub fn paint(cv: &mut Canvas<'_>, th: &Theme, fonts: &Fonts, g: &Glance<'_>, x: i32, y: i32) -> (i32, i32) {
    let w = W;
    let h = H;
    cv.round_rect(x, y, w, h, 16.0, th.surface);
    cv.stroke_round_rect(x, y, w, h, 16.0, 1.0, th.hairline);

    cv.text(
        fonts.semibold.as_ref(),
        g.date,
        x + 20,
        y + 36,
        14.0,
        th.text_dim,
    );
    cv.text(
        fonts.semibold.as_ref(),
        g.clock,
        x + 20,
        y + 78,
        36.0,
        th.text,
    );

    cv.text(
        fonts.semibold.as_ref(),
        "Project",
        x + 20,
        y + 118,
        13.0,
        th.text,
    );
    let mut ly = y + 140;
    for (i, line) in g.project_lines.iter().take(3).enumerate() {
        let mark = match i % 3 {
            0 => "●",
            1 => "■",
            _ => "○",
        };
        cv.text(
            fonts.regular.as_ref(),
            &format!("{mark}  {line}"),
            x + 20,
            ly,
            12.5,
            th.text_med,
        );
        ly += 20;
    }

    cv.text(
        fonts.semibold.as_ref(),
        "Environment",
        x + 20,
        y + 214,
        13.0,
        th.text,
    );
    let labels = ["Cloud", "Local", "Worktree"];
    let mut cx = x + 20;
    for (i, label) in labels.iter().enumerate() {
        let tw = text_width(fonts.regular.as_ref(), label, 12.0) + 20;
        let active = i == g.environment;
        let fill = if active {
            th.accent
        } else {
            Rgba::rgba(255, 255, 255, 18)
        };
        let fg = if active { th.on_accent } else { th.text_med };
        cv.round_rect(cx, y + 228, tw, 26, 13.0, fill);
        cv.text(
            fonts.semibold.as_ref(),
            label,
            cx + 10,
            y + 246,
            12.0,
            fg,
        );
        cx += tw + 8;
    }
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{Accent, ThemeKind};

    #[test]
    fn paints_without_panic() {
        let fonts = Fonts::load();
        let th = Theme::with(ThemeKind::Dark, Accent::Blue);
        let mut buf = vec![0u8; 400 * 320 * 4];
        let mut cv = Canvas::new(&mut buf, 400, 320, 1);
        let g = Glance {
            clock: "9:50",
            date: "TUE 17 NOV",
            project_lines: &["Infi-AI", "solyra-panel", "agent wrap"],
            environment: 1,
        };
        let (w, h) = paint(&mut cv, &th, &fonts, &g, 20, 20);
        assert_eq!((w, h), (W, H));
        assert_eq!(PROJECT_PLACEHOLDER.len(), 3);
    }
}

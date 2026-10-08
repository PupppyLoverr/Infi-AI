//! File-type icons for uitk apps, painted with egui shapes so they stay
//! crisp at any scale: a two-tone folder, and a folded-corner document
//! coloured by kind with a small white kind mark.

use egui::{vec2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Folder,
    Text,
    Code,
    Image,
    Pdf,
    Archive,
    Audio,
    Video,
    Other,
}

impl FileKind {
    pub fn of(name: &str, is_dir: bool) -> Self {
        if is_dir {
            return Self::Folder;
        }
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "txt" | "md" | "log" | "rst" | "org" | "csv" => Self::Text,
            "rs" | "py" | "js" | "ts" | "c" | "h" | "cpp" | "hpp" | "go" | "sh" | "toml"
            | "json" | "yaml" | "yml" | "html" | "css" | "lua" | "java" | "rb" | "xml" => {
                Self::Code
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" => Self::Image,
            "pdf" => Self::Pdf,
            "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "bz2" | "deb" | "tgz" => Self::Archive,
            "mp3" | "flac" | "wav" | "ogg" | "opus" | "m4a" => Self::Audio,
            "mp4" | "mkv" | "webm" | "mov" | "avi" => Self::Video,
            _ => Self::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::Text => "Text",
            Self::Code => "Source code",
            Self::Image => "Image",
            Self::Pdf => "PDF document",
            Self::Archive => "Archive",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Other => "Document",
        }
    }

    fn color(self) -> Color32 {
        let [r, g, b] = match self {
            Self::Folder => [0x4E, 0xA1, 0xFF],
            Self::Text => [0x7C, 0x86, 0x9C],
            Self::Code => [0x8B, 0x5C, 0xF6],
            Self::Image => cosmos_theme::semantic::SUCCESS,
            Self::Pdf => cosmos_theme::semantic::DANGER,
            Self::Archive => cosmos_theme::semantic::WARNING,
            Self::Audio => [0xEC, 0x48, 0x99],
            Self::Video => [0x63, 0x66, 0xF1],
            Self::Other => [0x9C, 0xA3, 0xAF],
        };
        Color32::from_rgb(r, g, b)
    }
}

/// Paint `kind`'s icon filling the square `rect`.
pub fn paint(painter: &Painter, rect: Rect, kind: FileKind) {
    let s = rect.width().min(rect.height());
    let o = rect.center() - vec2(s, s) / 2.0;
    let p = |x: f32, y: f32| -> Pos2 { o + vec2(x * s, y * s) };
    let base = kind.color();
    if kind == FileKind::Folder {
        let back = base.gamma_multiply(0.72);
        let r = CornerRadius::same((s * 0.12) as u8);
        painter.rect_filled(Rect::from_min_max(p(0.06, 0.16), p(0.48, 0.40)), r, back);
        painter.rect_filled(Rect::from_min_max(p(0.06, 0.24), p(0.94, 0.84)), r, back);
        painter.rect_filled(Rect::from_min_max(p(0.06, 0.32), p(0.94, 0.86)), r, base);
        return;
    }
    let fold = 0.26;
    let (l, t, r, b) = (0.16, 0.06, 0.84, 0.94);
    painter.add(Shape::convex_polygon(
        vec![p(l, t), p(r - fold, t), p(r, t + fold), p(r, b), p(l, b)],
        base,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        vec![p(r - fold, t), p(r - fold, t + fold), p(r, t + fold)],
        Color32::from_white_alpha(110),
        Stroke::NONE,
    ));
    let w = Stroke::new((s * 0.07).max(1.0), Color32::WHITE);
    match kind {
        FileKind::Code => {
            painter.line_segment([p(0.42, 0.50), p(0.32, 0.62)], w);
            painter.line_segment([p(0.32, 0.62), p(0.42, 0.74)], w);
            painter.line_segment([p(0.58, 0.50), p(0.68, 0.62)], w);
            painter.line_segment([p(0.68, 0.62), p(0.58, 0.74)], w);
        }
        FileKind::Image => {
            painter.add(Shape::convex_polygon(
                vec![p(0.26, 0.80), p(0.44, 0.56), p(0.58, 0.80)],
                Color32::WHITE,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![p(0.48, 0.80), p(0.62, 0.62), p(0.74, 0.80)],
                Color32::from_white_alpha(200),
                Stroke::NONE,
            ));
            painter.circle_filled(p(0.64, 0.46), s * 0.06, Color32::WHITE);
        }
        FileKind::Audio => {
            painter.line_segment([p(0.56, 0.44), p(0.56, 0.72)], w);
            painter.circle_filled(p(0.48, 0.74), s * 0.08, Color32::WHITE);
            painter.line_segment([p(0.56, 0.44), p(0.68, 0.50)], w);
        }
        FileKind::Video => {
            painter.add(Shape::convex_polygon(
                vec![p(0.40, 0.48), p(0.66, 0.62), p(0.40, 0.76)],
                Color32::WHITE,
                Stroke::NONE,
            ));
        }
        FileKind::Archive => {
            for i in 0..4 {
                let y = 0.30 + i as f32 * 0.12;
                let x = if i % 2 == 0 { 0.46 } else { 0.54 };
                painter.rect_filled(
                    Rect::from_min_size(p(x, y), vec2(s * 0.08, s * 0.08)),
                    0.0,
                    Color32::WHITE,
                );
            }
        }
        _ => {
            for (i, len) in [0.44, 0.44, 0.30].iter().enumerate() {
                let y = 0.54 + i as f32 * 0.12;
                painter.line_segment([p(0.30, y), p(0.30 + len, y)], w);
            }
        }
    }
}

/// Allocate an `size`-px square in the layout and paint the icon there.
pub fn show(ui: &mut egui::Ui, kind: FileKind, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect, kind);
    }
    resp
}

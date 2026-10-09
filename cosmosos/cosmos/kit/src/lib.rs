//! cosmos-kit — the structural component set every CosmosOS app builds
//! its chrome from (docs/kit.md). Components read colours from [`Kit`],
//! which resolves the live palette + wallpaper accent from config, so a
//! light/dark or accent change restyles every app on its next frame.

use cosmos_theme::{palette, Accent, Palette, Rgba};
use egui::Color32;

pub mod controls;
pub mod icons;

pub use icons::Icon;

/// Resolved theme for the current frame.
#[derive(Clone, Copy, Debug)]
pub struct Kit {
    pub dark: bool,
    pub p: &'static Palette,
    pub accent: Accent,
}

impl Kit {
    /// The kit for `ctx`, re-resolved from config.json whenever its
    /// mtime changes (cached in egui temp memory otherwise).
    pub fn get(ctx: &egui::Context) -> Kit {
        let id = egui::Id::new("cosmos-kit");
        let mtime = cosmos_uitk::theme::config_mtime();
        if let Some((m, kit)) = ctx.data(|d| d.get_temp::<(Option<std::time::SystemTime>, Kit)>(id))
        {
            if m == mtime {
                return kit;
            }
        }
        let dark = cosmos_uitk::theme::dark_from_config();
        let kit = Kit {
            dark,
            p: palette(dark),
            accent: Accent::new(cosmos_uitk::theme::accent_from_config(dark)),
        };
        ctx.data_mut(|d| d.insert_temp(id, (mtime, kit)));
        kit
    }

    pub fn text(&self) -> Color32 {
        c32(self.p.text)
    }
    pub fn text2(&self) -> Color32 {
        c32(self.p.text_secondary)
    }
    pub fn text3(&self) -> Color32 {
        c32(self.p.text_tertiary)
    }
    pub fn hairline(&self) -> Color32 {
        c32(self.p.hairline)
    }
    pub fn accent(&self) -> Color32 {
        rgb(self.accent.base)
    }
    pub fn accent_pressed(&self) -> Color32 {
        rgb(self.accent.pressed)
    }
    pub fn accent_tint(&self) -> Color32 {
        c32(self.accent.tint)
    }
    /// Neutral control fill for `state` (rest/hover/press), over any surface.
    pub fn fill(&self, state: State) -> Color32 {
        let a = match (state, self.dark) {
            (State::Rest, true) => 18,
            (State::Rest, false) => 12,
            (State::Hover, true) => 30,
            (State::Hover, false) => 22,
            (State::Press, true) => 42,
            (State::Press, false) => 32,
        };
        with_alpha(self.p.text, a)
    }
}

/// Interaction state shared by every component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Rest,
    Hover,
    Press,
}

impl State {
    pub fn of(resp: &egui::Response) -> State {
        if resp.is_pointer_button_down_on() {
            State::Press
        } else if resp.hovered() {
            State::Hover
        } else {
            State::Rest
        }
    }
}

pub fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}
pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}
pub fn with_alpha(c: Rgba, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

/// 2px accent focus ring drawn outside `rect`.
pub fn focus_ring(ui: &egui::Ui, kit: &Kit, rect: egui::Rect, radius: f32) {
    ui.painter().rect_stroke(
        rect.expand(2.0),
        radius + 2.0,
        egui::Stroke::new(2.0, kit.accent()),
        egui::StrokeKind::Outside,
    );
}

//! Icon painting: each [`Icon`] is a white 2× PNG rasterized from SVG at
//! build time, uploaded once per context and tinted at paint time, so
//! one asset serves every theme and state.

use egui::{Color32, Rect, Response, Sense, TextureHandle, Ui};

include!(concat!(env!("OUT_DIR"), "/icons_gen.rs"));

pub fn texture(ctx: &egui::Context, icon: Icon) -> TextureHandle {
    let id = egui::Id::new(("cosmos-kit-icon", icon));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let img = image::load_from_memory_with_format(icon.png(), image::ImageFormat::Png)
        .expect("build-time icon png")
        .to_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    );
    let tex = ctx.load_texture(
        format!("kit-icon-{}", icon.name()),
        ci,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Paint `icon` filling `rect`, tinted `color`.
pub fn paint(ui: &Ui, icon: Icon, rect: Rect, color: Color32) {
    let tex = texture(ui.ctx(), icon);
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter().image(tex.id(), rect, uv, color);
}

/// Allocate `size`×`size` and paint `icon` in it.
pub fn show(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    paint(ui, icon, rect, color);
    resp
}

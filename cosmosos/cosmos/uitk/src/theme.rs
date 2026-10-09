//! Cosmos visual identity for egui apps — every value comes from the
//! `cosmos-theme` tokens (spec-v3 §2): violet-tinted translucent
//! window fill, 8/12/14 radii, 4px spacing grid, 28px rows, accent
//! derived from the active wallpaper.

use cosmos_theme::{palette, radius, space, Accent, Rgba, ROW_H};
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn mix(c: Rgba, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], alpha)
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = palette(dark);
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let accent = Accent::new(accent_from_config(dark));

    v.panel_fill = c32(p.window);
    v.window_fill = c32(p.window);
    v.extreme_bg_color = if dark { c32(p.base) } else { c32(p.raised) };
    v.faint_bg_color = mix(p.text, if dark { 10 } else { 8 });
    v.code_bg_color = c32(p.raised);
    v.override_text_color = Some(c32(p.text));
    v.weak_text_color = Some(c32(p.text_secondary));
    v.hyperlink_color = rgb(accent.base);
    v.selection.bg_fill = c32(accent.tint);
    v.selection.stroke = Stroke::new(1.0, rgb(accent.base));
    v.window_stroke = Stroke::new(1.0, c32(p.hairline));

    let hair = c32(p.hairline);
    let fill = mix(p.text, if dark { 18 } else { 12 });
    let hover = mix(p.text, if dark { 28 } else { 20 });
    v.widgets.noninteractive.bg_fill = c32(p.window);
    v.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, hair);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c32(p.text_secondary));
    v.widgets.inactive.bg_fill = fill;
    v.widgets.inactive.weak_bg_fill = fill;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c32(p.text));
    v.widgets.hovered.bg_fill = hover;
    v.widgets.hovered.weak_bg_fill = hover;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c32(p.text));
    // Pressed/active = accent family; focused = 2px accent ring.
    v.widgets.active.bg_fill = rgb(accent.pressed);
    v.widgets.active.weak_bg_fill = c32(accent.tint);
    v.widgets.active.bg_stroke = Stroke::new(2.0, rgb(accent.base));
    v.widgets.active.fg_stroke = Stroke::new(1.0, c32(p.text));
    v.widgets.open = v.widgets.hovered;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(radius::CONTROL as u8);
        w.expansion = 0.0;
    }
    v.menu_corner_radius = CornerRadius::same(radius::PANEL as u8);
    v.window_corner_radius = CornerRadius::same(radius::WINDOW as u8);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.striped = false;
    v.slider_trailing_fill = true;

    ctx.set_visuals(v);
    ctx.all_styles_mut(|style| {
        use cosmos_theme::text;
        let f = |s: cosmos_theme::TextStyle| FontId::new(s.size, FontFamily::Proportional);
        style.text_styles = [
            (TextStyle::Small, f(text::CAPTION)),
            (TextStyle::Body, f(text::BODY)),
            (TextStyle::Button, f(text::BODY)),
            (TextStyle::Heading, f(text::TITLE2)),
            (
                TextStyle::Monospace,
                FontId::new(text::MONO.size, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(space::S8, space::S8);
        style.spacing.button_padding = egui::vec2(space::S12, space::S4);
        style.spacing.interact_size.y = ROW_H;
        style.spacing.window_margin = egui::Margin::same(space::S16 as i8);
        style.spacing.menu_margin = egui::Margin::same(space::S8 as i8);
        style.spacing.indent = space::S16;
        style.spacing.text_edit_width = 240.0;
    });
}

/// The compositor's authoritative config file.
fn config_text() -> String {
    let path = std::env::var("COSMOS_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let base = std::env::var("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
                        .join(".config")
                });
            base.join("cosmos/config.json")
        });
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The compositor's config.json, parsed (`Null` when missing or invalid).
pub fn compositor_config() -> serde_json::Value {
    serde_json::from_str(&config_text()).unwrap_or(serde_json::Value::Null)
}

/// Dark theme is the Cosmos default; the config file can flip it.
/// `appearance` from config.json — parsed, because a substring test
/// also matched the `"light"` key inside `accent_rgb` and forced every
/// app body light once the wallpaper accent was persisted.
pub fn dark_from_config() -> bool {
    serde_json::from_str::<serde_json::Value>(&config_text())
        .ok()
        .and_then(|v| v.get("appearance")?.as_str().map(|a| a != "light"))
        .unwrap_or(true)
}

/// The config file's mtime — frame loops poll this and re-apply the
/// theme when it changes so light/dark + accent reach running apps.
pub fn config_mtime() -> Option<std::time::SystemTime> {
    let path = std::env::var("COSMOS_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let base = std::env::var("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
                        .join(".config")
                });
            base.join("cosmos/config.json")
        });
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The accent rgb for this mode: the compositor-resolved `accent_rgb`
/// (includes the wallpaper-extracted "auto" accent), else the preset
/// table, else the theme fallback violet.
pub fn accent_from_config(dark: bool) -> [u8; 3] {
    let text = config_text();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return cosmos_theme::ACCENT_FALLBACK;
    };
    let mode = if dark { "dark" } else { "light" };
    if let Some(arr) = v
        .get("accent_rgb")
        .and_then(|a| a.get(mode))
        .and_then(|a| a.as_array())
    {
        let c: Vec<u8> = arr
            .iter()
            .filter_map(|x| x.as_u64().map(|n| n as u8))
            .collect();
        if c.len() == 3 {
            return [c[0], c[1], c[2]];
        }
    }
    match v.get("accent").and_then(|a| a.as_str()) {
        Some(name) if name != "auto" => cosmos_ipc::accent_rgb(name, dark),
        _ => cosmos_theme::ACCENT_FALLBACK,
    }
}

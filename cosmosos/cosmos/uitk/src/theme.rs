//! Cosmos visual identity for egui apps: restrained and dark-first, with a
//! single blue accent reserved for true active state (text selection,
//! hyperlinks, the pressed widget) — same rule as the compositor chrome.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub fn apply(ctx: &egui::Context, dark: bool) {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    let fg = if dark {
        Color32::from_gray(220)
    } else {
        Color32::from_gray(30)
    };
    let panel = if dark {
        Color32::from_gray(24)
    } else {
        Color32::from_gray(244)
    };
    let window = if dark {
        Color32::from_gray(30)
    } else {
        Color32::from_gray(250)
    };
    let extreme = if dark {
        Color32::from_gray(16)
    } else {
        Color32::from_gray(236)
    };

    v.panel_fill = panel;
    v.window_fill = window;
    v.extreme_bg_color = extreme;
    v.faint_bg_color = if dark {
        Color32::from_gray(36)
    } else {
        Color32::from_gray(232)
    };
    v.override_text_color = Some(fg);
    // The one Cosmos accent — the active preset's colour, read from
    // config.json so egui widgets match the shell's preset switch.
    let [ar, ag, ab] = accent_from_config(dark);
    let accent = Color32::from_rgb(ar, ag, ab);
    v.hyperlink_color = accent;

    // Selection carries the accent, washed out enough to keep text legible.
    v.selection.bg_fill = if dark {
        Color32::from_rgba_premultiplied(ar, ag, ab, 0x4D)
    } else {
        Color32::from_rgba_premultiplied(ar, ag, ab, 0x33)
    };
    v.selection.stroke = Stroke::new(1.0, accent);

    v.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(
        1.0,
        if dark {
            Color32::from_gray(52)
        } else {
            Color32::from_gray(210)
        },
    );
    v.widgets.inactive.weak_bg_fill = if dark {
        Color32::from_gray(46)
    } else {
        Color32::from_gray(226)
    };
    v.widgets.hovered.weak_bg_fill = if dark {
        Color32::from_gray(58)
    } else {
        Color32::from_gray(216)
    };
    // The pressed widget gets a whisper of accent — egui's "active" state
    // is momentary, so this stays subtle.
    v.widgets.active.weak_bg_fill = if dark {
        Color32::from_rgba_premultiplied(ar, ag, ab, 0x38)
    } else {
        Color32::from_rgba_premultiplied(ar, ag, ab, 0x28)
    };
    v.widgets.open.weak_bg_fill = v.widgets.hovered.weak_bg_fill;

    // Widget corners soften to match the shell's rounded-card language —
    // egui's default 2px reads sharp/technical against 12px cards.
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(4);
    }
    v.menu_corner_radius = CornerRadius::same(6);
    v.window_corner_radius = CornerRadius::same(6);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.window_stroke = Stroke::new(
        1.0,
        if dark {
            Color32::from_gray(58)
        } else {
            Color32::from_gray(200)
        },
    );

    ctx.set_visuals(v);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.window_margin = egui::Margin::same(10);
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

/// Dark theme is the Cosmos default; the config file can flip it.
pub fn dark_from_config() -> bool {
    !config_text().contains("\"light\"")
}

/// The accent preset's rgb for this mode (Azure when unset/unknown).
pub fn accent_from_config(dark: bool) -> [u8; 3] {
    let text = config_text();
    let name = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("accent")?.as_str().map(str::to_string))
        .unwrap_or_else(|| cosmos_ipc::DEFAULT_ACCENT.to_string());
    cosmos_ipc::accent_rgb(&name, dark)
}

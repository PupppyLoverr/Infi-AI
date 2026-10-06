//! Cosmos visual identity for egui apps: restrained, monochrome, dark-first.
//! No accent colours — selection and focus are expressed in greys, matching
//! the compositor chrome.

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
    v.hyperlink_color = fg;

    // Selection in monochrome — no brand accent.
    v.selection.bg_fill = if dark {
        Color32::from_gray(72)
    } else {
        Color32::from_gray(198)
    };
    v.selection.stroke = Stroke::new(1.0, fg);

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
    v.widgets.active.weak_bg_fill = if dark {
        Color32::from_gray(68)
    } else {
        Color32::from_gray(204)
    };
    v.widgets.open.weak_bg_fill = v.widgets.hovered.weak_bg_fill;

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

/// Dark theme is the Cosmos default; the config file can flip it.
pub fn dark_from_config() -> bool {
    let path = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
                .join(".config")
        })
        .join("cosmos/config.toml");
    std::fs::read_to_string(path)
        .map(|s| !s.contains("appearance = \"light\""))
        .unwrap_or(true)
}

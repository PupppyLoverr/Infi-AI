//! The Cosmos panel — a top menubar in the macOS idiom: Cosmos mark and
//! the focused application's name on the left; workspace pager and the
//! status tray (network · volume · battery · clock) on the right. Window
//! task management lives on the bottom dock.

use cosmic_text::Color as CtColor;
use tiny_skia::{Color, PixmapMut};

use crate::{draw, glass, icons, ShellState};

const LAUNCH_BTN_W: f64 = 46.0;
const APP_NAME_X: f64 = 54.0;
const WS_CELL_W: f64 = 16.0;

/// (glass fallback bg, hover, separator, fg, fg dim) from the v3 palette.
fn theme(dark: bool) -> (Color, Color, Color, CtColor, CtColor) {
    let p = cosmos_theme::palette(dark);
    let c = |v: cosmos_theme::Rgba| Color::from_rgba8(v[0], v[1], v[2], v[3]);
    let t = |v: cosmos_theme::Rgba| CtColor::rgba(v[0], v[1], v[2], v[3]);
    let hover = if dark {
        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x1F)
    } else {
        Color::from_rgba8(0x00, 0x00, 0x00, 0x14)
    };
    (
        c(p.window),
        hover,
        c(p.hairline),
        t(p.text),
        t(p.text_secondary),
    )
}

/// Left edge of the first menu title, just past the bold app name.
fn menus_x(name: &str) -> f64 {
    APP_NAME_X + 20.0 + name.len() as f64 * 8.0 + 6.0
}

/// Focused window's display name — the macOS menubar app-name slot.
/// Falls back to the window title, then nothing.
fn focused_app_name(state: &ShellState) -> String {
    let Some(win) = state.windows.iter().find(|w| w.focused) else {
        return String::new();
    };
    let id = icons::key_for(&win.app_id);
    state
        .apps
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.name.clone())
        .unwrap_or_else(|| {
            if win.title.is_empty() {
                String::new()
            } else {
                win.title.clone()
            }
        })
}

/// Width of the status column — shared by draw, click and `tray_clicked`.
/// Layout: [dnd?][net][vol][battery?] icons at 20px stride, then the clock text.
pub fn status_text_len(info: &crate::sysinfo::SysInfo, dnd: bool) -> f64 {
    let icons = 2.0
        + if dnd { 1.0 } else { 0.0 }
        + info
            .battery
            .as_ref()
            .map(|b| if b.present { 1.0 } else { 0.0 })
            .unwrap_or(0.0);
    8.0 + icons * 20.0 + 6.0 + info.clock.len() as f64 * 7.0
}

/// Repaint the panel.
pub fn draw(state: &mut ShellState) {
    let (w, h) = state.panel_size;
    if w == 0 {
        return;
    }
    let Some(layer) = state.panel.clone() else {
        return;
    };
    let (bg, hover_bg, sep, fg, fg_dim) = theme(state.dark);
    let (hx, hov) = state.panel_hover;
    let name = focused_app_name(state);
    let status_w = status_text_len(&state.sysinfo, state.dnd);
    let net_on = state.sysinfo.network.online;
    let vol_muted = state
        .sysinfo
        .volume
        .as_ref()
        .map(|v| v.muted)
        .unwrap_or(false);
    let bat_pct = state
        .sysinfo
        .battery
        .as_ref()
        .filter(|b| b.present)
        .map(|b| b.percent);
    let clock = state.sysinfo.clock.clone();
    let ws_count = crate::workspace_count(state);
    let workspaces = state.workspaces.clone();
    let dark = state.dark;
    let tray_x = w as f64 - status_w - 12.0;
    let pill = crate::island::pill_rect(state);
    let pill_act = crate::island::pill_activity(state);

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) = state.pool.create_buffer(
        w as i32,
        h as i32,
        stride,
        wayland_client::protocol::wl_shm::Format::Abgr8888,
    ) else {
        tracing::warn!("panel: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    pixmap.fill(Color::TRANSPARENT);
    glass::fill_glass(
        &mut pixmap,
        &crate::ShellState::wallpaper_name(),
        0.0,
        0.0,
        w as f32,
        h as f32,
        0.0,
        0.0,
        0.0,
        state.dark,
        bg,
    );

    // Cosmos mark — the launcher button.
    if hov && hx < LAUNCH_BTN_W {
        draw::fill_round_rect(
            &mut pixmap,
            4.0,
            4.0,
            LAUNCH_BTN_W as f32 - 8.0,
            h as f32 - 8.0,
            8.0,
            hover_bg,
        );
    }
    let glyph = Color::from_rgba8(fg.r(), fg.g(), fg.b(), fg.a());
    icons::icon(
        &mut pixmap,
        "start",
        LAUNCH_BTN_W as f32 / 2.0 - 9.0,
        h as f32 / 2.0 - 9.0,
        18.0,
        draw::accent(dark),
    );

    // Focused app — macOS menubar shows the app icon then its bold name.
    if !name.is_empty() {
        let icon_key = state
            .windows
            .iter()
            .find(|w| w.focused)
            .map(|w| icons::key_for(&w.app_id));
        if let Some(key) = icon_key {
            icons::icon(
                &mut pixmap,
                &key,
                APP_NAME_X as f32,
                h as f32 / 2.0 - 7.0,
                14.0,
                icons::tint_for(&key, dark).unwrap_or(glyph),
            );
        }
        draw::text_bold(
            &mut pixmap,
            APP_NAME_X as f32 + 20.0,
            h as f32 / 2.0 - 8.0,
            200.0,
            16.0,
            13.0,
            &name,
            fg,
        );
        // App menus — titles after the bold name; the open one highlighted.
        for (i, (tx, tw)) in crate::menubar::title_rects(menus_x(&name))
            .into_iter()
            .enumerate()
        {
            let hot = state.menu_open == Some(i) || (hov && hx >= tx && hx < tx + tw);
            if hot {
                draw::fill_round_rect(
                    &mut pixmap,
                    tx as f32,
                    4.0,
                    tw as f32,
                    h as f32 - 8.0,
                    6.0,
                    hover_bg,
                );
            }
            draw::text(
                &mut pixmap,
                tx as f32 + 10.0,
                h as f32 / 2.0 - 8.0,
                tw as f32,
                16.0,
                13.0,
                crate::menubar::TITLES[i],
                fg,
            );
        }
    }

    // Dynamic island pill — a centred capsule holding the latest
    // clipboard snippet + staged-file count; click/hover expands the
    // island card below the menubar (droppy-style).
    {
        let (px, py, pw, ph) = pill;
        // Keep clear of the focused-app name on the left and the pager on
        // the right — in tight widths the pill just isn't drawn.
        let name_right = if name.is_empty() {
            LAUNCH_BTN_W + 4.0
        } else {
            crate::menubar::titles_end(menus_x(&name)) + 12.0
        };
        let pager_left = w as f64 - status_w - 12.0 - ws_count as f64 * WS_CELL_W - 10.0;
        if px > name_right && px + pw < pager_left {
            let pill_bg = if state.island_open {
                draw::accent_soft(dark)
            } else if dark {
                Color::from_rgba8(0x24, 0x25, 0x2A, 0xD8)
            } else {
                Color::from_rgba8(0xED, 0xED, 0xF0, 0xE8)
            };
            let hover_pill = hov && hx >= px && hx < px + pw;
            let pill_bg = if hover_pill && !state.island_open {
                hover_bg
            } else {
                pill_bg
            };
            draw::fill_round_rect(
                &mut pixmap,
                px as f32,
                py as f32,
                pw as f32,
                ph as f32,
                (ph / 2.0) as f32,
                pill_bg,
            );
            if let Some(act) = &pill_act {
                // Live activity: status dot + label (approval, agent, media).
                draw::fill_round_rect(
                    &mut pixmap,
                    (px + 11.0) as f32,
                    (py + ph / 2.0 - 3.5) as f32,
                    7.0,
                    7.0,
                    3.5,
                    if act.live {
                        draw::accent(dark)
                    } else {
                        Color::from_rgba8(fg_dim.r(), fg_dim.g(), fg_dim.b(), 0xB0)
                    },
                );
                draw::text(
                    &mut pixmap,
                    (px + 24.0) as f32,
                    (py + ph / 2.0 - 7.0) as f32,
                    (pw - 32.0) as f32,
                    14.0,
                    11.5,
                    &act.label,
                    fg,
                );
            } else {
                let mut tx = px + 10.0;
                icons::icon(
                    &mut pixmap,
                    "clipboard",
                    tx as f32,
                    (py + (ph - 13.0) / 2.0) as f32,
                    13.0,
                    glyph,
                );
                tx += 18.0;
                if let Some(snip) = state.clip_history.front() {
                    let snippet: String = snip.chars().take(22).collect();
                    draw::text(
                        &mut pixmap,
                        tx as f32,
                        (py + ph / 2.0 - 7.0) as f32,
                        (pw - (tx - px) - 10.0) as f32,
                        14.0,
                        11.5,
                        &snippet.replace('\n', " "),
                        fg_dim,
                    );
                }
            }
            if pill_act.is_none() && !state.staged_files.is_empty() {
                let n = state.staged_files.len().to_string();
                draw::fill_round_rect(
                    &mut pixmap,
                    (px + pw - 20.0) as f32,
                    (py + (ph - 14.0) / 2.0) as f32,
                    14.0,
                    14.0,
                    7.0,
                    draw::accent(dark),
                );
                draw::text(
                    &mut pixmap,
                    (px + pw - 20.0) as f32 + 4.0,
                    (py + (ph - 14.0) / 2.0) as f32,
                    12.0,
                    12.0,
                    9.5,
                    &n,
                    CtColor::rgba(0xFF, 0xFF, 0xFF, 0xFF),
                );
            }
        }
    }

    // Right side: status icons + clock (rightmost — the tray), then the pager.
    if hov && hx >= tray_x {
        draw::fill_round_rect(
            &mut pixmap,
            tray_x as f32 + 2.0,
            4.0,
            status_w as f32 + 8.0,
            h as f32 - 8.0,
            8.0,
            hover_bg,
        );
    }
    // Tray icons then the clock — the Win11/macOS status-icons idiom.
    let mut ix = tray_x + 8.0;
    let icon_y = h as f32 / 2.0 - 7.0;
    // Focus (DND) moon — macOS shows it in the status area while on.
    if state.dnd {
        let moon = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(glyph),
            anti_alias: true,
            ..Default::default()
        };
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_circle(ix as f32 + 7.0, icon_y + 7.5, 6.0);
        pb.push_circle(ix as f32 + 10.0, icon_y + 5.0, 5.0);
        if let Some(path) = pb.finish() {
            pixmap.fill_path(
                &path,
                &moon,
                tiny_skia::FillRule::EvenOdd,
                tiny_skia::Transform::default(),
                None,
            );
        }
        ix += 20.0;
    }
    icons::icon(
        &mut pixmap,
        if net_on { "net-on" } else { "net-off" },
        ix as f32,
        icon_y,
        14.0,
        glyph,
    );
    ix += 20.0;
    icons::icon(
        &mut pixmap,
        if vol_muted { "vol-mute" } else { "vol-on" },
        ix as f32,
        icon_y,
        14.0,
        glyph,
    );
    ix += 20.0;
    if let Some(pct) = bat_pct {
        icons::battery(&mut pixmap, ix as f32, icon_y, 14.0, glyph, pct);
        ix += 20.0;
    }
    draw::text(
        &mut pixmap,
        ix as f32 + 2.0,
        h as f32 / 2.0 - 8.0,
        status_w as f32,
        16.0,
        12.0,
        &clock,
        fg,
    );
    let mut rx = tray_x - ws_count as f64 * WS_CELL_W - 10.0;

    // Workspace dots — the active one an accent capsule; occupied
    // workspaces read solid, empty ones faint.
    let cy = h as f32 / 2.0;
    for ws in &workspaces {
        let cx = rx as f32 + WS_CELL_W as f32 / 2.0;
        if hov && hx >= rx && hx < rx + WS_CELL_W {
            draw::fill_round_rect(&mut pixmap, cx - 7.0, cy - 7.0, 14.0, 14.0, 7.0, hover_bg);
        }
        if ws.focused {
            draw::fill_round_rect(
                &mut pixmap,
                cx - 6.0,
                cy - 3.0,
                12.0,
                6.0,
                3.0,
                draw::accent(dark),
            );
        } else {
            let c = if ws.window_count > 0 { fg } else { fg_dim };
            let alpha = if ws.window_count > 0 { 0xD0 } else { 0x70 };
            let dot = Color::from_rgba8(c.r(), c.g(), c.b(), alpha);
            draw::fill_round_rect(&mut pixmap, cx - 3.0, cy - 3.0, 6.0, 6.0, 3.0, dot);
        }
        rx += WS_CELL_W;
    }

    // Bottom hairline separator.
    draw::fill_rect(&mut pixmap, 0.0, h as f32 - 1.0, w as f32, 1.0, sep);

    use smithay_client_toolkit::shell::WaylandSurface;
    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    wl_surface.commit();
}

/// Hit-test + dispatch a click on the panel.
pub fn click(state: &mut ShellState, x: f64, _y: f64) -> bool {
    let (w, _) = state.panel_size;
    // Tray region toggles the quick-settings flyout. When the flyout was
    // just dismissed by the focus-loss `leave` of THIS same click, the
    // toggle must not fire — the click's job was already the dismissal.
    if state.tray_clicked(x) {
        let just_dismissed = state
            .quick_dismissed_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(300));
        state.quick_dismissed_at = None;
        if !just_dismissed {
            state.set_quick_open(!state.quick_open);
        }
        return true;
    }
    if state.quick_open {
        state.set_quick_open(false);
    }
    // App menu titles toggle their dropdown; any other panel press
    // closes an open one.
    let was_open = state.menu_open;
    let name = focused_app_name(state);
    if !name.is_empty() {
        for (i, (tx, tw)) in crate::menubar::title_rects(menus_x(&name))
            .into_iter()
            .enumerate()
        {
            if x >= tx && x < tx + tw {
                let next = (state.menu_open != Some(i)).then_some(i);
                state.set_menu(next, tx as i32);
                return true;
            }
        }
    }
    if state.menu_open.is_some() {
        state.set_menu(None, 0);
    }
    if x < LAUNCH_BTN_W {
        // The Cosmos mark opens the system menu; Start lives in the dock.
        let sys = crate::menubar::SYSTEM;
        if was_open != Some(sys) {
            state.set_menu(Some(sys), 4);
        }
        return true;
    }
    // Dynamic island pill — centred capsule toggles the island card.
    // Same just-dismissed guard as the tray: when the click itself moved
    // keyboard focus off an open card, `leave` already closed it.
    {
        let (px, _, pw, _) = crate::island::pill_rect(state);
        if x >= px && x < px + pw {
            let just_dismissed = state
                .island_dismissed_at
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(300));
            state.island_dismissed_at = None;
            if !just_dismissed {
                state.set_island_open(!state.island_open);
            }
            return true;
        }
    }
    // Workspace pager.
    let count = crate::workspace_count(state);
    let status_w = status_text_len(&state.sysinfo, state.dnd);
    let ws_start = (w as f64 - status_w - 12.0) - count as f64 * WS_CELL_W - 10.0;
    if x >= ws_start && x < ws_start + count as f64 * WS_CELL_W {
        let idx = ((x - ws_start) / WS_CELL_W) as usize;
        if idx < count {
            state.ipc.send(&cosmos_ipc::Request::SwitchWorkspace {
                workspace: idx as u8,
            });
            return true;
        }
    }
    false
}

/// Pointer hover — launcher button + tray highlight bookkeeping.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let _ = y;
    let prev = state.panel_hover;
    state.panel_hover = (x, true);
    let edge = |px: f64, w: f64| {
        px < LAUNCH_BTN_W || px >= w - status_text_len(&state.sysinfo, state.dnd) - 12.0
    };
    edge(prev.0, state.panel_size.0 as f64) != edge(x, state.panel_size.0 as f64)
}

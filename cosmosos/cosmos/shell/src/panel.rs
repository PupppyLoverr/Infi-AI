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
        // The bare desktop carries Finder's menus under its own name.
        return "Desktop".into();
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

/// Pager cap: occupied workspaces plus the current one, at most this many.
const MAX_WS: usize = 5;

/// Workspaces the pager shows: occupied ones plus the current one, at most
/// [`MAX_WS`], always keeping the current one in view.
pub fn visible_workspaces(all: &[cosmos_ipc::WorkspaceInfo]) -> Vec<cosmos_ipc::WorkspaceInfo> {
    let shown: Vec<_> = all
        .iter()
        .filter(|w| w.focused || w.window_count > 0)
        .cloned()
        .collect();
    if shown.len() <= MAX_WS {
        return shown;
    }
    let cur = shown.iter().position(|w| w.focused).unwrap_or(0);
    let start = cur.saturating_sub(MAX_WS / 2).min(shown.len() - MAX_WS);
    shown[start..start + MAX_WS].to_vec()
}

/// An agent is working right now — the tray shows the sparkle only then.
pub fn agent_running(state: &ShellState) -> bool {
    state.agents.iter().any(|a| a.busy && !a.stopped)
}

/// "NN%" beside the battery glyph when a battery is present.
fn battery_label(info: &crate::sysinfo::SysInfo) -> Option<String> {
    info.battery
        .as_ref()
        .filter(|b| b.present)
        .map(|b| format!("{}%", b.percent))
}

/// Width of the status column — shared by draw, click and `tray_clicked`.
/// Layout: [agent?][dnd?][net][vol][battery + %?][controls] icons at a
/// 20px stride, then the clock text.
pub fn status_text_len(state: &ShellState) -> f64 {
    let info = &state.sysinfo;
    let bat = battery_label(info);
    let icons = [agent_running(state), state.dnd, bat.is_some()]
        .iter()
        .filter(|on| **on)
        .count()
        + 3;
    let bat_w = bat
        .map(|b| draw::text_width(12.0, &b) as f64 + 6.0)
        .unwrap_or(0.0);
    8.0 + icons as f64 * 20.0 + bat_w + 6.0 + draw::text_width(13.0, &info.clock) as f64
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
    let (bg, hover_bg, _sep, fg, fg_dim) = theme(state.dark);
    let (hx, hov) = state.panel_hover;
    let name = focused_app_name(state);
    let titles = crate::menubar::titles(state);
    let status_w = status_text_len(state);
    let net_on = state.sysinfo.network.online;
    let net_wifi = state.sysinfo.network.wifi;
    let agent_on = agent_running(state);
    let bat_label = battery_label(&state.sysinfo);
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
    let workspaces = visible_workspaces(&state.workspaces);
    let ws_count = workspaces.len();
    let dark = state.dark;
    let tray_x = w as f64 - status_w - 12.0;
    let pill = crate::island::pill_rect(state);
    let pill_act = crate::island::pill(state);

    let (pw, ph) = draw::phys(w, h);
    let stride = pw as i32 * 4;
    let Ok((buffer, canvas)) = state.pool.create_buffer(
        pw as i32,
        ph as i32,
        stride,
        wayland_client::protocol::wl_shm::Format::Abgr8888,
    ) else {
        tracing::warn!("panel: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
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
        "cosmos",
        LAUNCH_BTN_W as f32 / 2.0 - 8.0,
        h as f32 / 2.0 - 8.0,
        16.0,
        glyph,
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
        for (i, (tx, tw)) in crate::menubar::title_rects(menus_x(&name), &titles)
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
                titles[i],
                fg,
            );
        }
    }

    // Dynamic island — black capsule hugging the menubar centre.
    {
        let (px, py, pw, ph) = pill;
        // Keep clear of the focused-app name on the left and the pager on
        // the right — in tight widths the capsule just isn't drawn.
        let name_right = if name.is_empty() {
            LAUNCH_BTN_W + 4.0
        } else {
            crate::menubar::titles_end(menus_x(&name), &titles) + 12.0
        };
        let pager_left = w as f64 - status_w - 12.0 - ws_count as f64 * WS_CELL_W - 10.0;
        if px > name_right && px + pw < pager_left {
            let hover_pill = hov && hx >= px && hx < px + pw;
            crate::island::paint_pill(
                &mut pixmap,
                &pill_act,
                (px, py, pw, ph),
                hover_pill,
                state.island_open,
                dark,
            );
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
    // Agent sparkle — only while an agent is working.
    if agent_on {
        icons::icon(&mut pixmap, "sparkle", ix as f32, icon_y, 14.0, glyph);
        ix += 20.0;
    }
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
                crate::draw::xf(),
                None,
            );
        }
        ix += 20.0;
    }
    icons::icon(
        &mut pixmap,
        match (net_on, net_wifi) {
            (false, _) => "net-off",
            (true, true) => "net-on",
            (true, false) => "ethernet",
        },
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
        if let Some(label) = &bat_label {
            let lw = draw::text_width(12.0, label);
            draw::text(
                &mut pixmap,
                ix as f32,
                h as f32 / 2.0 - 7.5,
                lw + 2.0,
                16.0,
                12.0,
                label,
                fg,
            );
            ix += lw as f64 + 6.0;
        }
    }
    icons::icon(&mut pixmap, "controls", ix as f32, icon_y, 14.0, glyph);
    ix += 20.0;
    draw::text(
        &mut pixmap,
        ix as f32 + 2.0,
        h as f32 / 2.0 - 8.0,
        status_w as f32,
        16.0,
        13.0,
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

    // Soft 8% hairline instead of a hard line under the bar.
    let hairline = if dark {
        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14)
    } else {
        Color::from_rgba8(0x00, 0x00, 0x00, 0x14)
    };
    draw::fill_rect(&mut pixmap, 0.0, h as f32 - 1.0, w as f32, 1.0, hairline);

    use smithay_client_toolkit::shell::WaylandSurface;
    let wl_surface = layer.wl_surface().clone();
    state.set_viewport(&wl_surface, w, h);
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
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
        let titles = crate::menubar::titles(state);
        for (i, (tx, tw)) in crate::menubar::title_rects(menus_x(&name), &titles)
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
    let shown = visible_workspaces(&state.workspaces);
    let count = shown.len();
    let status_w = status_text_len(state);
    let ws_start = (w as f64 - status_w - 12.0) - count as f64 * WS_CELL_W - 10.0;
    if x >= ws_start && x < ws_start + count as f64 * WS_CELL_W {
        let idx = ((x - ws_start) / WS_CELL_W) as usize;
        if idx < count {
            state.ipc.send(&cosmos_ipc::Request::SwitchWorkspace {
                workspace: shown[idx].id,
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
    let edge = |px: f64, w: f64| px < LAUNCH_BTN_W || px >= w - status_text_len(state) - 12.0;
    edge(prev.0, state.panel_size.0 as f64) != edge(x, state.panel_size.0 as f64)
}

#[cfg(test)]
mod tests {
    use cosmos_ipc::WorkspaceInfo;

    fn ws(id: u8, focused: bool, window_count: usize) -> WorkspaceInfo {
        WorkspaceInfo {
            id,
            focused,
            window_count,
        }
    }

    fn ids(all: &[WorkspaceInfo]) -> Vec<u8> {
        super::visible_workspaces(all)
            .iter()
            .map(|w| w.id)
            .collect()
    }

    #[test]
    fn pager_shows_occupied_and_current_only() {
        let all = [
            ws(0, false, 2),
            ws(1, false, 0),
            ws(2, true, 0),
            ws(3, false, 1),
        ];
        assert_eq!(ids(&all), [0, 2, 3]);
    }

    #[test]
    fn pager_caps_at_five_and_keeps_current() {
        let all: Vec<_> = (0..9).map(|i| ws(i, i == 8, 1)).collect();
        assert_eq!(ids(&all), [4, 5, 6, 7, 8]);
        let all: Vec<_> = (0..9).map(|i| ws(i, i == 0, 1)).collect();
        assert_eq!(ids(&all), [0, 1, 2, 3, 4]);
    }
}

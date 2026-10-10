//! cosmos-lock — the CosmosOS session locker.
//!
//! ext-session-lock client: while it holds the lock the compositor
//! composites only the wallpaper plus these surfaces and routes all
//! input here. Correct password → `unlock()` → exit; the compositor's
//! `unlock()` handler then restores normal rendering and focus.
//!
//! UI is egui rasterized into shm buffers with cosmos-uitk's painter so
//! the lock card shares the desktop's type, radii, and theme. PAM
//! authenticates against the `cosmos-lock` service (common-auth).

use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use egui::{Align2, Color32, FontId, Key, Pos2, Rect, RichText, Vec2};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

struct LockSurfaceEntry {
    surface: SessionLockSurface,
    output: wl_output::WlOutput,
    width: u32,
    height: u32,
    configured: bool,
}

#[allow(dead_code)] // some fields exist only to keep wayland objects alive
struct LockState {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    shm: Shm,
    seat_state: SeatState,
    output_state: OutputState,
    pool: SlotPool,
    session_lock_state: SessionLockState,
    session_lock: Option<SessionLock>,
    lock_surfaces: Vec<LockSurfaceEntry>,
    qh: QueueHandle<Self>,
    ctx: egui::Context,
    painter: cosmos_uitk::raster::Painter,
    pointer_pos: Pos2,
    pending: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    dirty: bool,
    start: Instant,
    exit: bool,
    user: String,
    password: String,
    error: Option<String>,
    failed_at: Option<Instant>,
    /// Blurred active wallpaper, the lock card's glass backdrop.
    wallpaper: Option<egui::TextureHandle>,
    lite: bool,
    /// XKB layout code shown bottom-right ("US").
    layout: String,
    power_open: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let conn = Connection::connect_to_env().context("wayland connect")?;
    let (globals, queue) = registry_queue_init(&conn).context("registry")?;
    let qh = queue.handle();

    let mut event_loop: EventLoop<LockState> = EventLoop::try_new().context("calloop")?;
    let handle = event_loop.handle();

    let compositor_state = CompositorState::bind(&globals, &qh).context("compositor")?;
    let shm = Shm::bind(&globals, &qh).context("shm")?;
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);
    let session_lock_state = SessionLockState::new::<LockState>(&globals, &qh);
    let pool = SlotPool::new(512 * 1024, &shm).context("slot pool")?;

    let ctx = egui::Context::default();
    cosmos_uitk::fonts::install(&ctx);
    cosmos_uitk::theme::apply(&ctx, cosmos_uitk::theme::dark_from_config());

    let cfg = config_text();
    let lite = cfg_bool(&cfg, "lite_mode");
    let wallpaper = (!lite).then(|| load_wallpaper(&ctx, &cfg)).flatten();

    let session_lock = session_lock_state
        .lock::<LockState>(&qh)
        .context("session lock denied")?;

    let mut state = LockState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        shm,
        seat_state,
        output_state,
        pool,
        session_lock_state,
        session_lock: Some(session_lock),
        lock_surfaces: Vec::new(),
        qh: qh.clone(),
        ctx,
        painter: cosmos_uitk::raster::Painter::default(),
        pointer_pos: Pos2::new(-10.0, -10.0),
        pending: Vec::new(),
        modifiers: egui::Modifiers::default(),
        dirty: false,
        start: Instant::now(),
        exit: false,
        user: std::env::var("USER").unwrap_or_else(|_| "user".into()),
        password: String::new(),
        error: None,
        failed_at: None,
        wallpaper,
        lite,
        layout: keyboard_layout(),
        power_open: false,
    };

    WaylandSource::new(conn.clone(), queue)
        .insert(handle.clone())
        .context("wayland source")?;

    let mut next_frame = Instant::now();
    while !state.exit {
        let wait = next_frame
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1))
            .min(Duration::from_secs(30));
        event_loop.dispatch(wait, &mut state).context("dispatch")?;

        let due = Instant::now() >= next_frame;
        let any_configured = state.lock_surfaces.iter().any(|s| s.configured);
        if any_configured && (state.dirty || !state.pending.is_empty() || due) {
            // Events are consumed once (first configured surface) — all
            // surfaces share the same LockState fields, so replaying
            // input per surface would double-apply typed text.
            let mut first = true;
            let mut repaint = Duration::from_secs(30);
            for i in 0..state.lock_surfaces.len() {
                if !state.lock_surfaces[i].configured {
                    continue;
                }
                let delay = state.frame_surface(i, first);
                repaint = repaint.min(delay);
                first = false;
            }
            state.dirty = false;
            next_frame = Instant::now() + repaint.max(Duration::from_millis(4));
        }
    }
    // `unlock()` only queues unlock_and_destroy; exiting without a
    // flush drops it, and a lock client that dies without unlocking
    // leaves the session locked (ext-session-lock §lock).
    conn.flush().context("flush unlock")?;
    let _ = conn.roundtrip();
    Ok(())
}

impl LockState {
    /// One egui pass rasterized into surface `idx`'s shm buffer.
    /// `with_events` is set for the first surface only.
    fn frame_surface(&mut self, idx: usize, with_events: bool) -> Duration {
        let (w, h) = {
            let entry = &self.lock_surfaces[idx];
            (entry.width, entry.height)
        };
        if w == 0 || h == 0 {
            return Duration::from_secs(30);
        }
        let size = egui::vec2(w as f32, h as f32);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(self.start.elapsed().as_secs_f64()),
            events: if with_events {
                std::mem::take(&mut self.pending)
            } else {
                Vec::new()
            },
            focused: true,
            max_texture_side: Some(2048),
            ..Default::default()
        };

        // Clone the Arc'd context so the closure can borrow `self` for
        // the shared password/error state.
        let ctx = self.ctx.clone();
        let mut out = ctx.run_ui(input, |ui| {
            draw_lock_ui(ui, self);
        });

        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                self.painter.set_texture(*id, delta);
            }
        }
        let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
        for id in &out.textures_delta.free {
            self.painter.free_texture(*id);
        }
        out.textures_delta.clear();

        let stride = w as i32 * 4;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
        else {
            tracing::warn!("cosmos-lock: shm alloc failed");
            return Duration::from_secs(1);
        };
        self.painter.paint(canvas, w, h, &prims, 1.0, [0, 0, 0, 0]);

        let wl_surface = self.lock_surfaces[idx].surface.wl_surface().clone();
        // Same release-gated attach as uitk: the slot stays busy until the
        // compositor releases the buffer, so no live frame can be torn.
        if let Err(e) = buffer.attach_to(&wl_surface) {
            tracing::warn!("cosmos-lock: attach failed: {e}");
        }
        wl_surface.damage_buffer(0, 0, w as i32, h as i32);
        wl_surface.commit();

        out.viewport_output
            .values()
            .map(|v| v.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX)
            .min(Duration::from_secs(30))
    }

    fn try_auth(&mut self) {
        if self.password.is_empty() {
            return;
        }
        match pam_auth(&self.user, &self.password) {
            Ok(()) => {
                if let Some(lock) = &self.session_lock {
                    lock.unlock();
                }
                self.session_lock = None;
                self.exit = true;
            }
            Err(_) => {
                self.password.clear();
                self.error = Some("Incorrect password".into());
                self.failed_at = Some(Instant::now());
            }
        }
        self.dirty = true;
    }
}

fn pam_auth(user: &str, password: &str) -> Result<()> {
    let mut auth = pam::Client::with_password("cosmos-lock").context("PAM cosmos-lock service")?;
    auth.conversation_mut().set_credentials(user, password);
    auth.authenticate().context("PAM authenticate")?;
    Ok(())
}

/// Wall-clock for the lock card — civil-from-days, no chrono dep.
fn clock_now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let day = secs.rem_euclid(86400);
    let (hh, mm) = (day / 3600, (day % 3600) / 60);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let months = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    (
        format!("{:02}:{:02}", hh, mm),
        format!("{} {}, {}", months[(m - 1) as usize], d, year),
    )
}

/// The whole lock UI — translucent scrim + centered card. Paints the same
/// on every output; sized by that surface's screen_rect.
fn config_text() -> String {
    let home = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
                .join(".config")
        });
    std::fs::read_to_string(home.join("cosmos/config.json")).unwrap_or_default()
}

/// `"key": "value"` from the config without a JSON dep.
fn cfg_str(text: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = text.find(&pat)? + pat.len();
    let rest = text[i..].trim_start_matches([' ', ':', '\t']);
    rest.strip_prefix('"')?
        .split('"')
        .next()
        .map(str::to_string)
}

fn cfg_bool(text: &str, key: &str) -> bool {
    let pat = format!("\"{key}\"");
    text.find(&pat).is_some_and(|i| {
        text[i + pat.len()..]
            .trim_start_matches([' ', ':', '\t', '\n'])
            .starts_with("true")
    })
}

/// The active wallpaper, downscaled and blurred once: sampled under the
/// card so it reads as glass over the compositor's wallpaper.
fn load_wallpaper(ctx: &egui::Context, cfg: &str) -> Option<egui::TextureHandle> {
    let name = cfg_str(cfg, "wallpaper").unwrap_or_else(|| "violet".to_string());
    let dark = cfg_str(cfg, "appearance").as_deref() != Some("light");
    let name = cosmos_ipc::wallpaper_for(&name, dark);
    let dir = std::env::var("COSMOS_WALLPAPER_DIR")
        .unwrap_or_else(|_| "/usr/share/cosmos/wallpapers".to_string());
    let img = ["1920x1080", "3840x2160"].iter().find_map(|r| {
        image::open(std::path::Path::new(&dir).join(format!("{name}-{r}.png"))).ok()
    })?;
    let small = image::imageops::resize(
        &img.to_rgba8(),
        480,
        270,
        image::imageops::FilterType::Triangle,
    );
    let blurred = image::imageops::blur(&small, 8.0);
    let ci = egui::ColorImage::from_rgba_unmultiplied([480, 270], blurred.as_raw());
    Some(ctx.load_texture("lock-wallpaper", ci, egui::TextureOptions::LINEAR))
}

/// The compositor builds its keymap from XKB_DEFAULT_LAYOUT (xkbcommon
/// default "us"); Debian records the console layout in /etc/default/keyboard.
fn keyboard_layout() -> String {
    std::env::var("XKB_DEFAULT_LAYOUT")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/default/keyboard")
                .ok()?
                .lines()
                .find_map(|l| l.strip_prefix("XKBLAYOUT="))
                .map(|v| v.trim_matches('"').to_string())
        })
        .and_then(|l| l.split(',').next().map(str::to_string))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "us".to_string())
        .to_uppercase()
}

fn power(method: &str) {
    let r = zbus::blocking::Connection::system().and_then(|c| {
        c.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            method,
            &(false,),
        )
        .map(|_| ())
    });
    if let Err(e) = r {
        tracing::warn!("cosmos-lock: {method} failed: {e}");
    }
}

const CARD_W: f32 = 380.0;
const CARD_R: u8 = 22;
const FIELD_W: f32 = 300.0;
const FIELD_H: f32 = 36.0;
const SHAKE: f32 = 0.45;

/// Horizontal wrong-password shake: 6 Hz, decaying to rest over SHAKE s.
fn shake_offset(since: f32) -> f32 {
    if !(0.0..SHAKE).contains(&since) {
        return 0.0;
    }
    12.0 * (since * std::f32::consts::TAU * 6.0).sin() * (1.0 - since / SHAKE)
}

fn draw_lock_ui(ui: &mut egui::Ui, state: &mut LockState) {
    let rect = ui.max_rect();
    let ctx = ui.ctx().clone();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::ZERO,
        Color32::from_black_alpha(56),
    );

    let (clock, date) = clock_now();
    let top = rect.height() * 0.17;
    ui.painter().text(
        Pos2::new(rect.center().x, top - 58.0),
        Align2::CENTER_CENTER,
        date,
        FontId::proportional(22.0),
        Color32::from_white_alpha(230),
    );
    ui.painter().text(
        Pos2::new(rect.center().x, top + 16.0),
        Align2::CENTER_CENTER,
        clock,
        FontId::proportional(96.0),
        Color32::WHITE,
    );

    let since = state
        .failed_at
        .map_or(f32::MAX, |t| t.elapsed().as_secs_f32());
    let dx = if state.lite { 0.0 } else { shake_offset(since) };
    if dx != 0.0 || since < SHAKE {
        ctx.request_repaint();
    }

    let card_h = 24.0 + 72.0 + 14.0 + 28.0 + 18.0 + FIELD_H + 10.0 + 18.0 + 22.0;
    let card = Rect::from_center_size(
        Pos2::new(rect.center().x + dx, rect.height() * 0.58),
        Vec2::new(CARD_W, card_h),
    );
    let painter = ui.painter().clone();
    match &state.wallpaper {
        Some(tex) if !state.lite => {
            let uv = Rect::from_min_max(
                Pos2::new(card.min.x / rect.width(), card.min.y / rect.height()),
                Pos2::new(card.max.x / rect.width(), card.max.y / rect.height()),
            );
            painter.add(
                egui::epaint::RectShape::filled(card, CARD_R, Color32::WHITE)
                    .with_texture(tex.id(), uv),
            );
            painter.rect_filled(
                card,
                CARD_R,
                Color32::from_rgba_unmultiplied(0x1C, 0x17, 0x30, 120),
            );
        }
        _ => {
            painter.rect_filled(card, CARD_R, Color32::from_rgb(0x1E, 0x1B, 0x2A));
        }
    }
    painter.rect_stroke(
        card,
        CARD_R,
        egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
        egui::StrokeKind::Inside,
    );

    // Avatar: a person glyph in a 72px disc — no letter avatars.
    let mut y = card.min.y + 24.0;
    let av = Pos2::new(card.center().x, y + 36.0);
    painter.circle_filled(av, 36.0, Color32::from_white_alpha(38));
    painter.circle_stroke(
        av,
        36.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(46)),
    );
    cosmos_kit::icons::paint(
        ui,
        cosmos_kit::Icon::Person,
        Rect::from_center_size(av, Vec2::splat(40.0)),
        Color32::WHITE,
    );
    y += 72.0 + 14.0;
    painter.text(
        Pos2::new(card.center().x, y + 14.0),
        Align2::CENTER_CENTER,
        &state.user,
        FontId::proportional(22.0),
        Color32::WHITE,
    );
    y += 28.0 + 18.0;

    // Glass pill field: centred hint text or bullets, arrow inside on the right.
    let field = Rect::from_center_size(
        Pos2::new(card.center().x, y + FIELD_H / 2.0),
        Vec2::new(FIELD_W, FIELD_H),
    );
    painter.rect_filled(field, FIELD_H / 2.0, Color32::from_white_alpha(30));
    painter.rect_stroke(
        field,
        FIELD_H / 2.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(52)),
        egui::StrokeKind::Inside,
    );
    let arrow_d = FIELD_H - 8.0;
    let arrow = Rect::from_center_size(
        Pos2::new(field.max.x - 4.0 - arrow_d / 2.0, field.center().y),
        Vec2::splat(arrow_d),
    );
    // Symmetric insets keep the text optically centred on the field.
    let text_rect = field.shrink2(Vec2::new(arrow_d + 10.0, 0.0));
    let pwd_id = egui::Id::new("pwd");
    let mut submit = false;
    egui::Area::new(egui::Id::new("lock-field"))
        .fixed_pos(text_rect.min)
        .show(&ctx, |ui| {
            ui.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            let resp = ui.put(
                Rect::from_min_size(text_rect.min, text_rect.size()),
                egui::TextEdit::singleline(&mut state.password)
                    .id(pwd_id)
                    .password(true)
                    .frame(egui::Frame::NONE)
                    .vertical_align(egui::Align::Center)
                    .horizontal_align(egui::Align::Center)
                    .desired_width(text_rect.width())
                    .font(FontId::proportional(15.0))
                    .hint_text(
                        RichText::new("Enter Password").color(Color32::from_white_alpha(150)),
                    ),
            );
            if !ctx.memory(|m| m.has_focus(pwd_id)) {
                ctx.memory_mut(|m| m.request_focus(pwd_id));
            }
            let enter = ui.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            ..
                        }
                    )
                })
            });
            submit |= (enter || resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                && !state.password.is_empty();
            if resp.changed() {
                state.error = None;
            }
        });
    let ready = !state.password.is_empty();
    let arrow_resp = ui.interact(arrow, egui::Id::new("lock-arrow"), egui::Sense::click());
    painter.circle_filled(
        arrow.center(),
        arrow_d / 2.0,
        Color32::from_white_alpha(match (ready, arrow_resp.hovered()) {
            (true, true) => 96,
            (true, false) => 64,
            _ => 26,
        }),
    );
    cosmos_kit::icons::paint(
        ui,
        cosmos_kit::Icon::ArrowRight,
        Rect::from_center_size(arrow.center(), Vec2::splat(16.0)),
        Color32::from_white_alpha(if ready { 255 } else { 120 }),
    );
    submit |= arrow_resp.clicked() && ready;
    y += FIELD_H + 10.0;
    if let Some(err) = &state.error {
        painter.text(
            Pos2::new(card.center().x, y + 9.0),
            Align2::CENTER_CENTER,
            err,
            FontId::proportional(12.5),
            Color32::from_rgb(255, 150, 140),
        );
    }
    if submit {
        state.try_auth();
    }

    // Bottom-right: keyboard layout, then power with Restart / Shut Down.
    let base = Pos2::new(rect.max.x - 28.0, rect.max.y - 28.0);
    let pw = Rect::from_center_size(Pos2::new(base.x - 12.0, base.y - 12.0), Vec2::splat(32.0));
    let pw_resp = ui.interact(pw, egui::Id::new("lock-power"), egui::Sense::click());
    if pw_resp.hovered() || state.power_open {
        painter.circle_filled(pw.center(), 16.0, Color32::from_white_alpha(38));
    }
    cosmos_kit::icons::paint(
        ui,
        cosmos_kit::Icon::Power,
        Rect::from_center_size(pw.center(), Vec2::splat(20.0)),
        Color32::WHITE,
    );
    if pw_resp.clicked() {
        state.power_open = !state.power_open;
    }
    let kb_x = pw.min.x - 20.0;
    let galley = painter.layout_no_wrap(
        state.layout.clone(),
        FontId::proportional(13.0),
        Color32::from_white_alpha(230),
    );
    let text_pos = Pos2::new(
        kb_x - galley.size().x,
        pw.center().y - galley.size().y / 2.0,
    );
    painter.galley(text_pos, galley, Color32::WHITE);
    cosmos_kit::icons::paint(
        ui,
        cosmos_kit::Icon::Keyboard,
        Rect::from_center_size(
            Pos2::new(text_pos.x - 16.0, pw.center().y),
            Vec2::splat(20.0),
        ),
        Color32::WHITE,
    );
    if state.power_open {
        let menu = Rect::from_min_max(
            Pos2::new(pw.max.x - 150.0, pw.min.y - 8.0 - 2.0 * 30.0 - 12.0),
            Pos2::new(pw.max.x, pw.min.y - 8.0),
        );
        painter.rect_filled(
            menu,
            10,
            Color32::from_rgba_unmultiplied(0x1C, 0x17, 0x30, 235),
        );
        painter.rect_stroke(
            menu,
            10,
            egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
            egui::StrokeKind::Inside,
        );
        for (i, (label, method)) in [("Restart", "Reboot"), ("Shut Down", "PowerOff")]
            .into_iter()
            .enumerate()
        {
            let row = Rect::from_min_size(
                Pos2::new(menu.min.x + 6.0, menu.min.y + 6.0 + i as f32 * 30.0),
                Vec2::new(menu.width() - 12.0, 30.0),
            );
            let r = ui.interact(
                row,
                egui::Id::new(("lock-power-row", i)),
                egui::Sense::click(),
            );
            if r.hovered() {
                painter.rect_filled(row, 6, Color32::from_white_alpha(30));
            }
            painter.text(
                Pos2::new(row.min.x + 10.0, row.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(13.0),
                Color32::WHITE,
            );
            if r.clicked() {
                state.power_open = false;
                power(method);
            }
        }
        if ui.input(|i| i.pointer.any_pressed())
            && !pw_resp.clicked()
            && !ui.rect_contains_pointer(menu)
        {
            state.power_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake_decays_to_rest() {
        assert_eq!(shake_offset(f32::MAX), 0.0);
        assert_eq!(shake_offset(SHAKE), 0.0);
        assert!(shake_offset(0.04).abs() > 4.0);
        assert!(shake_offset(0.0).abs() < 1e-3);
    }

    #[test]
    fn config_lookups() {
        let c = r#"{"wallpaper": "peach", "lite_mode": true, "dark": false}"#;
        assert_eq!(cfg_str(c, "wallpaper").as_deref(), Some("peach"));
        assert!(cfg_bool(c, "lite_mode"));
        assert!(!cfg_bool(c, "dark"));
        assert!(!cfg_bool(c, "missing"));
    }
}

fn map_key(sym: Keysym) -> Option<Key> {
    use egui::Key::*;
    Some(match sym {
        Keysym::Escape => Escape,
        Keysym::Tab => Tab,
        Keysym::BackSpace => Backspace,
        Keysym::Return | Keysym::KP_Enter => Enter,
        Keysym::space => Space,
        Keysym::Delete => Delete,
        Keysym::Insert => Insert,
        Keysym::Home => Home,
        Keysym::End => End,
        Keysym::Page_Up => PageUp,
        Keysym::Page_Down => PageDown,
        Keysym::Left => ArrowLeft,
        Keysym::Right => ArrowRight,
        Keysym::Up => ArrowUp,
        Keysym::Down => ArrowDown,
        other => {
            let raw = other.raw();
            match raw {
                0x61..=0x7a | 0x41..=0x5a => {
                    return Key::from_name(
                        char::from_u32(raw)?
                            .to_ascii_uppercase()
                            .to_string()
                            .as_str(),
                    );
                }
                0x30..=0x39 => return Key::from_name(char::from_u32(raw)?.to_string().as_str()),
                _ => return None,
            }
        }
    })
}

fn map_modifiers(m: Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        mac_cmd: false,
        command: m.ctrl,
    }
}

fn key_event(event: &KeyEvent, pressed: bool, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    let mut ev = Vec::new();
    if let Some(key) = map_key(event.keysym) {
        ev.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
    }
    if pressed {
        if let Some(text) = event.utf8.as_deref() {
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            if !text.is_empty() {
                ev.push(egui::Event::Text(text));
            }
        }
    }
    ev
}

impl CompositorHandler for LockState {
    fn scale_factor_changed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _f: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _t: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _t: u32,
    ) {
    }
    fn surface_enter(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _o: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _o: &wl_output::WlOutput,
    ) {
    }
}

impl SessionLockHandler for LockState {
    /// Lock engaged — cover every output with a lock surface.
    fn locked(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, session_lock: SessionLock) {
        for output in self.output_state.outputs() {
            let surface = self.compositor_state.create_surface(qh);
            let lock_surface = session_lock.create_lock_surface::<Self>(surface, &output, qh);
            self.lock_surfaces.push(LockSurfaceEntry {
                surface: lock_surface,
                output,
                width: 0,
                height: 0,
                configured: false,
            });
        }
        tracing::info!("cosmos-lock: {} lock surface(s)", self.lock_surfaces.len());
    }

    /// Compositor refused or tore down the lock — exit (compositor's own
    /// unlock path restores the session).
    fn finished(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _lock: SessionLock) {
        std::process::exit(0);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _serial: u32,
    ) {
        let (w, h) = configure.new_size;
        for entry in &mut self.lock_surfaces {
            if entry.surface.wl_surface() == surface.wl_surface() {
                entry.width = w;
                entry.height = h;
                entry.configured = true;
            }
        }
        self.dirty = true;
    }
}

impl SeatHandler for LockState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _c: &Connection, _q: &QueueHandle<Self>, _s: wl_seat::WlSeat) {}
    fn remove_seat(&mut self, _c: &Connection, _q: &QueueHandle<Self>, _s: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard => {
                let _ = self.seat_state.get_keyboard(qh, &seat, None);
            }
            Capability::Pointer => {
                let _ = self.seat_state.get_pointer(qh, &seat);
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: wl_seat::WlSeat,
        _cap: Capability,
    ) {
    }
}

impl KeyboardHandler for LockState {
    fn enter(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _k: &wl_keyboard::WlKeyboard,
        _s: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        self.dirty = true;
    }
    fn leave(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _k: &wl_keyboard::WlKeyboard,
        _s: &wl_surface::WlSurface,
        _serial: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _k: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.pending.extend(key_event(&event, true, self.modifiers));
    }
    fn repeat_key(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _k: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.pending.extend(key_event(&event, true, self.modifiers));
    }
    fn release_key(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _k: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.pending
            .extend(key_event(&event, false, self.modifiers));
    }
    fn update_modifiers(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _k: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw: RawModifiers,
        _layout: u32,
    ) {
        self.modifiers = map_modifiers(modifiers);
    }
}

impl PointerHandler for LockState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            match ev.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.pointer_pos = Pos2::new(ev.position.0 as f32, ev.position.1 as f32);
                    self.pending
                        .push(egui::Event::PointerMoved(self.pointer_pos));
                }
                PointerEventKind::Leave { .. } => {
                    self.pending.push(egui::Event::PointerGone);
                }
                PointerEventKind::Press { button, .. }
                | PointerEventKind::Release { button, .. } => {
                    let button = match button {
                        0x110 => egui::PointerButton::Primary,
                        0x111 => egui::PointerButton::Secondary,
                        0x112 => egui::PointerButton::Middle,
                        _ => continue,
                    };
                    self.pending.push(egui::Event::PointerButton {
                        pos: self.pointer_pos,
                        button,
                        pressed: matches!(ev.kind, PointerEventKind::Press { .. }),
                        modifiers: self.modifiers,
                    });
                }
                _ => {}
            }
        }
    }
}

impl OutputHandler for LockState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _c: &Connection, _q: &QueueHandle<Self>, _o: wl_output::WlOutput) {}
    fn update_output(&mut self, _c: &Connection, _q: &QueueHandle<Self>, _o: wl_output::WlOutput) {}
    fn output_destroyed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        o: wl_output::WlOutput,
    ) {
        self.lock_surfaces.retain(|s| s.output != o);
    }
}

impl ShmHandler for LockState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for LockState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers!(OutputState, SeatState);
}

smithay_client_toolkit::delegate_dispatch2!(LockState);
smithay_client_toolkit::delegate_registry!(LockState);

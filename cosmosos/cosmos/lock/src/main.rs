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
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
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
        let Ok((buffer, canvas)) = self
            .pool
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
    let mut auth =
        pam::Client::with_password("cosmos-lock").context("PAM cosmos-lock service")?;
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
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ];
    (
        format!("{:02}:{:02}", hh, mm),
        format!("{} {}, {}", months[(m - 1) as usize], d, year),
    )
}

/// The whole lock UI — translucent scrim + centered card. Paints the same
/// on every output; sized by that surface's screen_rect.
fn draw_lock_ui(ui: &mut egui::Ui, state: &mut LockState) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, Color32::from_black_alpha(140));

    // Clock upper-third.
    let (clock, date) = clock_now();
    ui.painter().text(
        Pos2::new(rect.center().x, rect.height() * 0.22),
        Align2::CENTER_CENTER,
        clock,
        FontId::proportional(72.0),
        Color32::WHITE,
    );
    ui.painter().text(
        Pos2::new(rect.center().x, rect.height() * 0.22 + 52.0),
        Align2::CENTER_CENTER,
        date,
        FontId::proportional(18.0),
        Color32::from_white_alpha(170),
    );

    let ctx = ui.ctx().clone();
    egui::Area::new(egui::Id::new("lock-card"))
        .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, rect.height() * 0.06))
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(170))
                .stroke(egui::Stroke::new(1.0, Color32::from_white_alpha(30)))
                .corner_radius(16.0)
                .inner_margin(28.0)
                .show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.vertical_centered(|ui| {
                        // Avatar ring + username.
                        let (_resp, painter) =
                            ui.allocate_painter(Vec2::splat(64.0), egui::Sense::hover());
                        let c = painter.clip_rect().center();
                        let accent = ui.visuals().hyperlink_color;
                        painter.circle_filled(c, 30.0, accent.gamma_multiply(0.3));
                        painter.circle_stroke(c, 30.0, egui::Stroke::new(1.5, accent));
                        painter.text(
                            c,
                            Align2::CENTER_CENTER,
                            state
                                .user
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_ascii_uppercase()
                                .to_string(),
                            FontId::proportional(26.0),
                            Color32::WHITE,
                        );
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(&state.user)
                                .size(18.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.add_space(14.0);

                        let pwd_id = egui::Id::new("pwd");
                        let resp = ui.add_sized(
                            Vec2::new(300.0, 40.0),
                            egui::TextEdit::singleline(&mut state.password)
                                .id(pwd_id)
                                .password(true)
                                .desired_width(300.0)
                                .font(FontId::proportional(15.0))
                                .hint_text("Password"),
                        );
                        if !ctx.memory(|m| m.has_focus(pwd_id)) {
                            ctx.memory_mut(|m| m.request_focus(pwd_id));
                        }

                        ui.add_space(6.0);
                        let hint = if let Some(err) = &state.error {
                            RichText::new(err.clone()).color(Color32::from_rgb(255, 120, 110))
                        } else {
                            RichText::new("Press Return to unlock")
                                .color(Color32::from_white_alpha(120))
                        };
                        ui.label(hint.size(12.5));

                        let enter =
                            resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let typed_return = ui.input(|i| {
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
                        if enter || (typed_return && !state.password.is_empty()) {
                            state.try_auth();
                        }
                    });
                });
        });
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
                    )
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
    fn output_destroyed(&mut self, _c: &Connection, _q: &QueueHandle<Self>, o: wl_output::WlOutput) {
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

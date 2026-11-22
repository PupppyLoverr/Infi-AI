//! `solyra-panel lock`: the lock screen.
//!
//! Uses ext-session-lock (the compositor guarantees nothing else is shown or
//! gets input while locked, and keeps the session locked if this process dies)
//! and checks the password with PAM (`/etc/pam.d/solyra-lock`). Keys go
//! through the real keyboard layout (xkb), so any layout and non-ASCII
//! passwords work.
//!
//! Four layouts (`lock_style` in shell.conf):
//! * **Solyra** – blurred wallpaper under a dark scrim, time in the middle, a
//!   single password pill.
//! * **Centred** – large date and time near the top, the person's picture,
//!   name and password field near the bottom (macOS-like).
//! * **Clock corner** – a sharp wallpaper with a big clock in the lower left;
//!   any key slides in a centred sign-in card over a blurred backdrop
//!   (Windows-like).
//! * **Dashboard** – large clock, nL / now-playing capsule, avatar row, agents
//!   glance, empty calendar line, and optional media / weather chips.

use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_registry, delegate_seat,
    delegate_session_lock, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel::{channel, Event as ChannelEvent},
            timer::{TimeoutAction, Timer},
            EventLoop,
        },
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
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
    protocol::{
        wl_keyboard::WlKeyboard, wl_output::WlOutput, wl_seat::WlSeat, wl_shm::Format,
        wl_surface::WlSurface,
    },
    Connection, QueueHandle,
};

use crate::canvas::{decode_png, text_width, Canvas, Fonts, Icon, Rgba};
use crate::state::ShellPrefs;
use crate::tokens::{Accent, Theme, ThemeKind};
use solyra_conf::look::LockStyle;

const PAM_SERVICE: &str = "solyra-lock";
/// After this many failures in a row, wait before the next try.
const FAILS_BEFORE_DELAY: u32 = 3;

struct LockSurface {
    lock: SessionLockSurface,
    output: WlOutput,
    width: u32,
    height: u32,
    /// The blurred and sharp backdrops at this size (logical), cached.
    backdrop: Option<(u32, u32, Vec<u8>, Vec<u8>)>,
}

struct App {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    shm: Shm,
    pool: SlotPool,
    lock_state: SessionLockState,
    lock: Option<SessionLock>,
    surfaces: Vec<LockSurface>,
    keyboard: Option<WlKeyboard>,
    conn: Connection,
    qh: QueueHandle<App>,

    fonts: Fonts,
    theme: Theme,
    style: LockStyle,
    clock_fmt: &'static str,
    wallpaper: Option<Icon>,
    user: String,
    name: String,

    password: String,
    caps: bool,
    checking: bool,
    message: Option<String>,
    fails: u32,
    not_before: Option<Instant>,
    /// Clock corner: the sign-in card is showing.
    card: bool,
    last_key: Instant,
    auth_tx:
        Option<smithay_client_toolkit::reexports::calloop::channel::Sender<Result<(), String>>>,
    exit: bool,
}

/// Run the lock screen until the right password is entered.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh: QueueHandle<App> = event_queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;

    let prefs = ShellPrefs::load();
    let accent = Accent::parse(&prefs.look.accent).unwrap_or(Accent::Blue);
    // The lock screen is always dark: it sits over a darkened backdrop.
    let theme = Theme::with(ThemeKind::Dark, accent);
    let user = crate::pam::current_user();
    let name = crate::pam::full_name(
        &user,
        &std::fs::read_to_string("/etc/passwd").unwrap_or_default(),
    );
    let wallpaper_path = prefs
        .wallpaper_file
        .clone()
        .unwrap_or_else(|| prefs.wallpaper.path());
    let wallpaper = std::fs::read(&wallpaper_path)
        .ok()
        .and_then(|b| decode_png(&b));

    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new(4 * 1024 * 1024, &shm)?;
    let (auth_tx, auth_rx) = channel::<Result<(), String>>();
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh)?,
        shm,
        pool,
        lock_state: SessionLockState::new(&globals, &qh),
        lock: None,
        surfaces: Vec::new(),
        keyboard: None,
        conn: conn.clone(),
        qh: qh.clone(),
        fonts: Fonts::load(),
        theme,
        style: prefs.look.lock,
        clock_fmt: prefs.look.clock_format(),
        wallpaper,
        user,
        name,
        password: String::new(),
        caps: false,
        checking: false,
        message: None,
        fails: 0,
        not_before: None,
        card: false,
        last_key: Instant::now(),
        auth_tx: Some(auth_tx),
        exit: false,
    };

    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle())?;
    event_loop
        .handle()
        .insert_source(auth_rx, |ev, _, app| {
            if let ChannelEvent::Msg(result) = ev {
                app.auth_done(result);
            }
        })
        .map_err(|e| format!("{e}"))?;
    // The clock: redraw every second (and drop back from the card when idle).
    event_loop
        .handle()
        .insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, app| {
            if app.style == LockStyle::Corner
                && app.card
                && app.password.is_empty()
                && app.last_key.elapsed() > Duration::from_secs(20)
            {
                app.card = false;
            }
            app.draw_all();
            TimeoutAction::ToDuration(Duration::from_secs(1))
        })
        .map_err(|e| format!("{e}"))?;

    // Learn the outputs, then lock and cover each one.
    event_loop.dispatch(Duration::from_millis(200), &mut app)?;
    app.lock = Some(
        app.lock_state
            .lock(&qh)
            .map_err(|e| format!("ext-session-lock is not available: {e}"))?,
    );
    for output in app.outputs.outputs() {
        app.add_surface(output);
    }

    while !app.exit {
        event_loop.dispatch(Duration::from_millis(250), &mut app)?;
    }
    Ok(())
}

impl App {
    fn add_surface(&mut self, output: WlOutput) {
        let Some(lock) = self.lock.as_ref() else {
            return;
        };
        let surface = self.compositor.create_surface(&self.qh);
        let lock = lock.create_lock_surface(surface, &output, &self.qh);
        self.surfaces.push(LockSurface {
            lock,
            output,
            width: 0,
            height: 0,
            backdrop: None,
        });
    }

    fn scale_for(&self, output: &WlOutput) -> u32 {
        self.outputs
            .info(output)
            .map(|i| i.scale_factor.clamp(1, 4) as u32)
            .unwrap_or(1)
    }

    fn submit(&mut self) {
        if self.checking || self.password.is_empty() {
            return;
        }
        if let Some(t) = self.not_before {
            if Instant::now() < t {
                self.message = Some("Too many attempts. Wait a moment.".into());
                return;
            }
        }
        self.checking = true;
        self.message = None;
        let (user, pass) = (self.user.clone(), std::mem::take(&mut self.password));
        let tx = self.auth_tx.clone();
        std::thread::spawn(move || {
            let result = crate::pam::authenticate(PAM_SERVICE, &user, &pass);
            let mut pass = pass;
            wipe(&mut pass);
            if let Some(tx) = tx {
                let _ = tx.send(result);
            }
        });
        self.draw_all();
    }

    fn auth_done(&mut self, result: Result<(), String>) {
        self.checking = false;
        match result {
            Ok(()) => {
                if let Some(lock) = self.lock.take() {
                    lock.unlock();
                }
                let _ = self.conn.roundtrip();
                self.exit = true;
            }
            Err(e) => {
                self.fails += 1;
                self.message = Some(e);
                if self.fails >= FAILS_BEFORE_DELAY {
                    self.not_before = Some(
                        Instant::now()
                            + Duration::from_secs(
                                5 * u64::from(self.fails - FAILS_BEFORE_DELAY + 1),
                            ),
                    );
                }
                self.draw_all();
            }
        }
    }

    fn draw_all(&mut self) {
        for i in 0..self.surfaces.len() {
            self.draw(i);
        }
    }

    fn draw(&mut self, i: usize) {
        let (w, h) = (self.surfaces[i].width, self.surfaces[i].height);
        if w == 0 || h == 0 {
            return;
        }
        let scale = self.scale_for(&self.surfaces[i].output);
        let (pw, ph) = (w * scale, h * scale);
        // Backdrops are built at physical size once per size.
        if self.surfaces[i].backdrop.as_ref().map(|b| (b.0, b.1)) != Some((pw, ph)) {
            let sharp = backdrop(self.wallpaper.as_ref(), pw, ph);
            let blurred = blur(&sharp, pw, ph);
            self.surfaces[i].backdrop = Some((pw, ph, sharp, blurred));
        }
        let Ok((buffer, bytes)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, (pw * 4) as i32, Format::Argb8888)
        else {
            return;
        };
        let (_, _, sharp, blurred) = self.surfaces[i].backdrop.as_ref().expect("built above");
        let use_sharp = self.style == LockStyle::Corner && !self.card;
        bytes.copy_from_slice(if use_sharp { sharp } else { blurred });
        {
            let mut cv = Canvas::new_keep(bytes, w, h, scale as i32);
            let date = if self.style == LockStyle::Dashboard {
                chrono::Local::now().format("%a %-d %b").to_string().to_uppercase()
            } else {
                chrono::Local::now().format("%A %-d %B").to_string()
            };
            let view = View {
                theme: &self.theme,
                fonts: &self.fonts,
                style: self.style,
                clock: chrono::Local::now().format(self.clock_fmt).to_string(),
                date,
                name: &self.name,
                dots: self.password.chars().count(),
                caps: self.caps,
                checking: self.checking,
                message: self.message.as_deref(),
                card: self.card,
                agents_running: 0,
                media_title: None,
                media_artist: None,
                weather: None,
            };
            paint(&mut cv, &view);
        }
        let surface = self.surfaces[i].lock.wl_surface();
        surface.set_buffer_scale(scale as i32);
        if buffer.attach_to(surface).is_ok() {
            surface.damage_buffer(0, 0, pw as i32, ph as i32);
            surface.commit();
        }
    }
}

/// Overwrite a password in memory before it is freed.
fn wipe(s: &mut String) {
    // SAFETY: writing zero bytes keeps the string valid UTF-8.
    unsafe {
        for b in s.as_bytes_mut() {
            std::ptr::write_volatile(b, 0);
        }
    }
    s.clear();
}

/// Premultiplied BGRA of the wallpaper, scaled to cover `w`×`h`; a dark
/// gradient when there is no wallpaper.
pub fn backdrop(img: Option<&Icon>, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    match img {
        Some(ic) if ic.width > 0 && ic.height > 0 => {
            let s = (w as f32 / ic.width as f32).max(h as f32 / ic.height as f32);
            let ox = (ic.width as f32 * s - w as f32) / 2.0;
            let oy = (ic.height as f32 * s - h as f32) / 2.0;
            for y in 0..h {
                let sy = (((y as f32 + oy) / s) as u32).min(ic.height - 1);
                for x in 0..w {
                    let sx = (((x as f32 + ox) / s) as u32).min(ic.width - 1);
                    let si = ((sy * ic.width + sx) * 4) as usize;
                    let di = ((y * w + x) * 4) as usize;
                    out[di] = ic.pixels[si + 2];
                    out[di + 1] = ic.pixels[si + 1];
                    out[di + 2] = ic.pixels[si];
                    out[di + 3] = 255;
                }
            }
        }
        _ => {
            // Neutral graphite fallback — never purple-tinted.
            for y in 0..h {
                let t = y as f32 / h.max(1) as f32;
                let v = ((0x1c as f32 * (1.0 - t) + 0x0c as f32 * t) as u8).max(8);
                for x in 0..w {
                    let di = ((y * w + x) * 4) as usize;
                    out[di..di + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
        }
    }
    out
}

/// A soft blur: downscale by 8, three box-blur passes, scale back up with
/// bilinear filtering. Cheap enough to run once per screen size.
pub fn blur(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let f = 8u32;
    let (sw, sh) = ((w / f).max(1), (h / f).max(1));
    let mut small = vec![0f32; (sw * sh * 3) as usize];
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0f32; 3];
            let mut n = 0f32;
            for yy in (y * f)..((y + 1) * f).min(h) {
                for xx in (x * f)..((x + 1) * f).min(w) {
                    let i = ((yy * w + xx) * 4) as usize;
                    acc[0] += src[i] as f32;
                    acc[1] += src[i + 1] as f32;
                    acc[2] += src[i + 2] as f32;
                    n += 1.0;
                }
            }
            let o = ((y * sw + x) * 3) as usize;
            for c in 0..3 {
                small[o + c] = acc[c] / n.max(1.0);
            }
        }
    }
    let r = 2i32;
    for _ in 0..3 {
        let prev = small.clone();
        for y in 0..sh as i32 {
            for x in 0..sw as i32 {
                let mut acc = [0f32; 3];
                let mut n = 0f32;
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (xx, yy) = (
                            (x + dx).clamp(0, sw as i32 - 1),
                            (y + dy).clamp(0, sh as i32 - 1),
                        );
                        let i = ((yy as u32 * sw + xx as u32) * 3) as usize;
                        acc[0] += prev[i];
                        acc[1] += prev[i + 1];
                        acc[2] += prev[i + 2];
                        n += 1.0;
                    }
                }
                let o = ((y as u32 * sw + x as u32) * 3) as usize;
                for c in 0..3 {
                    small[o + c] = acc[c] / n;
                }
            }
        }
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let fy = ((y as f32 + 0.5) / f as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as u32, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) / f as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as u32, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            let at = |xx: u32, yy: u32, c: usize| small[((yy * sw + xx) * 3) as usize + c];
            let di = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
                let bot = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
                out[di + c] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
            out[di + 3] = 255;
        }
    }
    out
}

/// What the lock screen shows; pure data so it can be previewed.
pub struct View<'a> {
    pub theme: &'a Theme,
    pub fonts: &'a Fonts,
    pub style: LockStyle,
    pub clock: String,
    pub date: String,
    pub name: &'a str,
    pub dots: usize,
    pub caps: bool,
    pub checking: bool,
    pub message: Option<&'a str>,
    pub card: bool,
    /// Dashboard glance: agents currently running.
    pub agents_running: usize,
    pub media_title: Option<&'a str>,
    pub media_artist: Option<&'a str>,
    /// Short weather label, e.g. `25°`.
    pub weather: Option<&'a str>,
}

fn centred(
    cv: &mut Canvas<'_>,
    font: Option<&ab_glyph::FontRef<'_>>,
    t: &str,
    cx: i32,
    baseline: i32,
    size: f32,
    color: Rgba,
) {
    let tw = text_width(font, t, size);
    cv.text(font, t, cx - tw / 2, baseline, size, color);
}

fn avatar(cv: &mut Canvas<'_>, v: &View<'_>, cx: i32, cy: i32, r: f32) {
    cv.circle(cx as f32, cy as f32, r, v.theme.accent);
    let initial = v
        .name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let size = r * 1.05;
    let tw = text_width(v.fonts.semibold.as_ref(), &initial, size);
    cv.text(
        v.fonts.semibold.as_ref(),
        &initial,
        cx - tw / 2,
        cy + (size * 0.36) as i32,
        size,
        v.theme.on_accent,
    );
}

/// Frosted glass capsule: translucent fill, lit top edge, soft rim.
fn glass_pill(cv: &mut Canvas<'_>, x: i32, y: i32, w: i32, h: i32, r: f32) {
    cv.round_rect(x, y, w, h, r, Rgba::rgba(0x1c, 0x1c, 0x1e, 120));
    cv.stroke_round_rect(x, y, w, h, r, 1.0, Rgba::rgba(255, 255, 255, 64));
    let inset = (r * 0.55).round() as i32;
    cv.fill_rect(
        x + inset,
        y + 1,
        (w - 2 * inset).max(0),
        1,
        Rgba::rgba(255, 255, 255, 78),
    );
}

/// The password field: dots for typed characters, never the characters.
fn field(cv: &mut Canvas<'_>, v: &View<'_>, cx: i32, y: i32, w: i32) {
    let th = v.theme;
    let h = 40;
    let x = cx - w / 2;
    glass_pill(cv, x, y, w, h, 20.0);
    if v.checking {
        centred(
            cv,
            v.fonts.regular.as_ref(),
            "Checking…",
            cx,
            y + 26,
            13.0,
            th.text,
        );
    } else if v.dots == 0 {
        centred(
            cv,
            v.fonts.regular.as_ref(),
            "Enter Password",
            cx,
            y + 26,
            13.0,
            Rgba::rgba(255, 255, 255, 170),
        );
    } else {
        let n = v.dots.min(24) as i32;
        let gap = 14;
        let start = cx - (n - 1) * gap / 2;
        for i in 0..n {
            cv.circle((start + i * gap) as f32, (y + h / 2) as f32, 4.0, th.text);
        }
    }
    let mut note_y = y + h + 24;
    if let Some(m) = v.message {
        centred(
            cv,
            v.fonts.semibold.as_ref(),
            m,
            cx,
            note_y,
            13.0,
            Rgba::rgb(0xff, 0xb4, 0xa8),
        );
        note_y += 20;
    }
    if v.caps {
        centred(
            cv,
            v.fonts.regular.as_ref(),
            "Caps Lock is on",
            cx,
            note_y,
            12.0,
            Rgba::rgba(255, 255, 255, 200),
        );
    }
}

/// Paint the lock screen over the backdrop already in the canvas.
pub fn paint(cv: &mut Canvas<'_>, v: &View<'_>) {
    let (w, h) = (cv.width as i32, cv.height as i32);
    let white = Rgba::rgb(0xff, 0xff, 0xff);
    let soft = Rgba::rgba(255, 255, 255, 215);
    let big = |h: i32| (h as f32 * 0.13).clamp(64.0, 150.0);
    match v.style {
        LockStyle::Solyra => {
            // Neutral dark veil — wallpaper (Beach) stays readable underneath.
            cv.fill_rect(0, 0, w, h, Rgba::rgba(0, 0, 0, 88));
            let cy = h / 2 - 40;
            centred(
                cv,
                v.fonts.semibold.as_ref(),
                &v.clock,
                w / 2,
                cy,
                big(h) * 0.8,
                white,
            );
            centred(
                cv,
                v.fonts.regular.as_ref(),
                &v.date,
                w / 2,
                cy + 36,
                18.0,
                soft,
            );
            field(cv, v, w / 2, cy + 80, 300);
            centred(
                cv,
                v.fonts.regular.as_ref(),
                v.name,
                w / 2,
                cy + 74,
                13.0,
                soft,
            );
        }
        LockStyle::Classic => {
            cv.fill_rect(0, 0, w, h, Rgba::rgba(0, 0, 0, 60));
            let top = (h as f32 * 0.12) as i32;
            centred(
                cv,
                v.fonts.semibold.as_ref(),
                &v.date,
                w / 2,
                top + 20,
                20.0,
                soft,
            );
            centred(
                cv,
                v.fonts.semibold.as_ref(),
                &v.clock,
                w / 2,
                top + 20 + big(h) as i32,
                big(h),
                white,
            );
            let by = h - 220;
            avatar(cv, v, w / 2, by, 34.0);
            centred(
                cv,
                v.fonts.semibold.as_ref(),
                v.name,
                w / 2,
                by + 62,
                16.0,
                white,
            );
            field(cv, v, w / 2, by + 80, 240);
        }
        LockStyle::Corner => {
            if v.card {
                cv.fill_rect(0, 0, w, h, Rgba::rgba(0, 0, 0, 90));
                let cy = h / 2 - 60;
                avatar(cv, v, w / 2, cy, 64.0);
                centred(
                    cv,
                    v.fonts.semibold.as_ref(),
                    v.name,
                    w / 2,
                    cy + 104,
                    26.0,
                    white,
                );
                field(cv, v, w / 2, cy + 130, 300);
            } else {
                // A gentle shade under the corner clock so it reads on any wallpaper.
                for i in 0..160 {
                    let a = (i as f32 / 160.0 * 110.0) as u8;
                    cv.fill_rect(0, h - 160 + i, w, 1, Rgba::rgba(0, 0, 0, a));
                }
                let size = big(h) * 1.05;
                cv.text(
                    v.fonts.semibold.as_ref(),
                    &v.clock,
                    48,
                    h - 120,
                    size,
                    white,
                );
                cv.text(v.fonts.regular.as_ref(), &v.date, 52, h - 72, 24.0, soft);
                cv.text(
                    v.fonts.regular.as_ref(),
                    "Press any key to sign in",
                    52,
                    h - 40,
                    13.0,
                    Rgba::rgba(255, 255, 255, 170),
                );
            }
        }
        LockStyle::Dashboard => {
            // Soft neutral veil so the Beach wallpaper and blur stay visible.
            cv.fill_rect(0, 0, w, h, Rgba::rgba(0, 0, 0, 72));
            let edge = Rgba::rgba(255, 255, 255, 56);

            // Weather sits top-right, clear of the centre capsule.
            if let Some(weather) = v.weather.filter(|t| !t.is_empty()) {
                let size = 14.0;
                let tw = text_width(v.fonts.semibold.as_ref(), weather, size);
                let (pw, ph) = (tw + 28, 30);
                let (px, py) = (w - pw - 28, 20);
                glass_pill(cv, px, py, pw, ph, 15.0);
                cv.text(
                    v.fonts.semibold.as_ref(),
                    weather,
                    px + 14,
                    py + 20,
                    size,
                    white,
                );
            }

            // Top centre: now-playing title, or the nL brand box.
            if let Some(title) = v.media_title.filter(|t| !t.is_empty()) {
                let size = 13.0;
                let max_tw = (w / 2).clamp(120, 280);
                let tw = text_width(v.fonts.semibold.as_ref(), title, size).min(max_tw);
                let (pw, ph) = (tw + 36, 32);
                let px = (w - pw) / 2;
                glass_pill(cv, px, 20, pw, ph, 16.0);
                cv.text_truncated(
                    v.fonts.semibold.as_ref(),
                    title,
                    px + 18,
                    41,
                    size,
                    max_tw,
                    white,
                );
            } else {
                let mh = 28;
                let mw = crate::canvas::nl_mark_width(mh, v.fonts);
                cv.mark_nl((w - mw) / 2, 20, mh, v.fonts);
            }

            // Large date above a larger clock, in the upper half.
            let clock_size = big(h);
            let clock_base = (h as f32 * 0.40) as i32;
            let date_size = (clock_size * 0.30).clamp(22.0, 34.0);
            let date_base = clock_base - (clock_size * 0.82) as i32 - 6;
            centred(
                cv,
                v.fonts.semibold.as_ref(),
                &v.date,
                w / 2,
                date_base,
                date_size,
                soft,
            );
            centred(
                cv,
                v.fonts.semibold.as_ref(),
                &v.clock,
                w / 2,
                clock_base,
                clock_size,
                white,
            );

            // Glance, then avatars, then the password. Flow so they don't collide.
            let mut y = clock_base + 22;
            if v.agents_running > 0 {
                let label = format!("{} Agents RUNNING", v.agents_running);
                let size = 12.0;
                let tw = text_width(v.fonts.semibold.as_ref(), &label, size);
                let (aw, ah) = (tw + 28, 26);
                let ax = (w - aw) / 2;
                glass_pill(cv, ax, y, aw, ah, 13.0);
                centred(
                    cv,
                    v.fonts.semibold.as_ref(),
                    &label,
                    w / 2,
                    y + 18,
                    size,
                    white,
                );
                y += 38;
            }

            centred(
                cv,
                v.fonts.regular.as_ref(),
                "No Events Scheduled Today",
                w / 2,
                y + 16,
                14.0,
                Rgba::rgba(255, 255, 255, 170),
            );
            y += 30;

            let title = v.media_title.unwrap_or("").trim();
            let artist = v.media_artist.unwrap_or("").trim();
            if !title.is_empty() || !artist.is_empty() {
                let (mw, mh) = (300, 56);
                let mx = (w - mw) / 2;
                glass_pill(cv, mx, y, mw, mh, 16.0);
                let text_w = mw - 118;
                if !title.is_empty() {
                    cv.text_truncated(
                        v.fonts.semibold.as_ref(),
                        title,
                        mx + 16,
                        y + 24,
                        13.0,
                        text_w,
                        white,
                    );
                }
                if !artist.is_empty() {
                    cv.text_truncated(
                        v.fonts.regular.as_ref(),
                        artist,
                        mx + 16,
                        y + 42,
                        12.0,
                        text_w,
                        soft,
                    );
                }
                // Decorative only: the lock screen does not control playback.
                let gy = (y + mh / 2) as f32;
                let gx = (mx + mw - 24) as f32;
                cv.icon_skip(gx - 52.0, gy, 12.0, false, white);
                cv.icon_pause(gx - 26.0, gy, 13.0, white);
                cv.icon_skip(gx, gy, 12.0, true, white);
                y += mh + 16;
            }

            let avatar_r = 28.0;
            let avatar_cy = (y + 40).max(h - 196).min(h - 148);
            let spread = 48;
            let ax = w / 2 - spread;
            let px = w / 2 + spread;
            avatar(cv, v, ax, avatar_cy, avatar_r);
            cv.circle(
                px as f32,
                avatar_cy as f32,
                avatar_r,
                Rgba::rgba(0x1c, 0x1c, 0x1e, 140),
            );
            cv.ring(px as f32, avatar_cy as f32, avatar_r, 1.4, edge);
            let arm = avatar_r * 0.42;
            cv.line(
                px as f32 - arm,
                avatar_cy as f32,
                px as f32 + arm,
                avatar_cy as f32,
                2.0,
                white,
            );
            cv.line(
                px as f32,
                avatar_cy as f32 - arm,
                px as f32,
                avatar_cy as f32 + arm,
                2.0,
                white,
            );
            let label_base = avatar_cy + avatar_r as i32 + 18;
            centred(
                cv,
                v.fonts.regular.as_ref(),
                v.name,
                ax,
                label_base,
                12.0,
                soft,
            );
            centred(
                cv,
                v.fonts.semibold.as_ref(),
                "NEW",
                px,
                label_base,
                11.0,
                white,
            );
            field(cv, v, w / 2, label_base + 16, 260);
        }
    }
}

impl SessionLockHandler for App {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {}

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        // The compositor refused (another locker is running): nothing to do.
        eprintln!("solyra-panel lock: the session could not be locked");
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        if let Some(i) = self
            .surfaces
            .iter()
            .position(|s| s.lock.wl_surface() == surface.wl_surface())
        {
            self.surfaces[i].width = w;
            self.surfaces[i].height = h;
            self.draw(i);
        }
    }
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: &WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: &WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.last_key = Instant::now();
        if self.style == LockStyle::Corner && !self.card {
            // The first key only reveals the sign-in card.
            self.card = true;
            self.draw_all();
            return;
        }
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter => self.submit(),
            Keysym::BackSpace => {
                self.password.pop();
            }
            Keysym::Escape => {
                wipe(&mut self.password);
                self.message = None;
            }
            _ => {
                if let Some(t) = event.utf8 {
                    if !self.checking && self.password.chars().count() < 256 {
                        self.password.extend(t.chars().filter(|c| !c.is_control()));
                    }
                }
            }
        }
        self.draw_all();
    }

    fn repeat_key(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        kb: &WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::BackSpace {
            self.press_key(conn, qh, kb, serial, event);
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        if m.caps_lock != self.caps {
            self.caps = m.caps_lock;
            self.draw_all();
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            match self.seats.get_keyboard(qh, &seat, None) {
                Ok(k) => self.keyboard = Some(k),
                Err(e) => eprintln!("solyra-panel lock: no keyboard: {e}"),
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: i32,
    ) {
        self.draw_all();
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        // A display plugged in while locked is covered too.
        if self.lock.is_some() {
            self.add_surface(output);
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        self.surfaces.retain(|s| s.output != output);
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_session_lock!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_registry!(App);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_clears_the_password() {
        let mut s = String::from("hunter2");
        wipe(&mut s);
        assert!(s.is_empty());
    }

    #[test]
    fn backdrop_covers_and_blur_softens() {
        let ic = Icon {
            width: 4,
            height: 2,
            pixels: (0..32).map(|i| if i % 8 < 4 { 255 } else { 0 }).collect(),
        };
        let b = backdrop(Some(&ic), 40, 40);
        assert_eq!(b.len(), 40 * 40 * 4);
        assert!(b.chunks_exact(4).all(|p| p[3] == 255), "opaque everywhere");
        let bl = blur(&b, 40, 40);
        assert_eq!(bl.len(), b.len());
        // Blurring a hard stripe pattern leaves fewer extreme values.
        let extremes = |v: &[u8]| {
            v.chunks_exact(4)
                .filter(|p| p[0] == 0 || p[0] == 255)
                .count()
        };
        assert!(extremes(&bl) < extremes(&b));
        let fallback = backdrop(None, 10, 10);
        assert!(fallback.chunks_exact(4).all(|p| p[3] == 255));
    }
}

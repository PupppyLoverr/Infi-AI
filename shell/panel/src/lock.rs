//! `solyra-panel lock`: the lock screen.
//!
//! Uses ext-session-lock (the compositor guarantees nothing else is shown or
//! gets input while locked, and keeps the session locked if this process dies)
//! and checks the password with PAM (`/etc/pam.d/solyra-lock`). Keys go
//! through the real keyboard layout (xkb), so any layout and non-ASCII
//! passwords work.
//!
//! Four layouts (`lock_style` in shell.conf):
//! * **Solyra** – soft-blurred wallpaper under a light neutral veil, the clock
//!   in the upper half, a single password pill.
//! * **Classic** – sharp wallpaper, a large clock in the top third with the
//!   date above it, avatar and password near the bottom (macOS-like).
//! * **Clock corner** – a sharp wallpaper with a big clock in the lower left;
//!   any key slides in a centred sign-in card over a blurred backdrop
//!   (Windows-like).
//! * **Dashboard** – a calm glance: huge clock, one date line, a weather glass
//!   chip, and either the nL mark or a now-playing capsule. Live weather,
//!   media and agent counts come from the same places the panel uses.

use std::path::Path;
use std::sync::{Arc, Mutex};
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

use crate::canvas::{decode_png, text_width, text_width_tracked, Canvas, Fonts, Icon, Rgba};
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
    /// Latest MPRIS player, filled by [`crate::media::spawn_watcher`].
    media: Arc<Mutex<Option<crate::media::Media>>>,
    /// Normalized first line of `weather.txt`, when the file has one.
    weather: Option<String>,
    /// Non-terminal agent tasks, when a runtime directory is present.
    agents_running: usize,
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
    let media = Arc::new(Mutex::new(None));
    // The lock process does not share the panel's memory. Poll MPRIS the same
    // way the panel does; a missing session bus leaves the slot empty.
    crate::media::spawn_watcher(media.clone(), || {});
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
        media,
        weather: None,
        agents_running: 0,
        last_key: Instant::now(),
        auth_tx: Some(auth_tx),
        exit: false,
    };
    app.refresh_live();

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
            app.refresh_live();
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

    /// Re-read weather and the agent runtime. Media updates itself in `self.media`.
    fn refresh_live(&mut self) {
        self.weather = load_weather();
        self.agents_running = live_agents(crate::agent::root_dir().as_deref());
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
            let mut blurred = blur(&sharp, pw, ph);
            vivid(&mut blurred, 1.25);
            self.surfaces[i].backdrop = Some((pw, ph, sharp, blurred));
        }
        let Ok((buffer, bytes)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, (pw * 4) as i32, Format::Argb8888)
        else {
            return;
        };
        let (_, _, sharp, blurred) = self.surfaces[i].backdrop.as_ref().expect("built above");
        let use_sharp = sharp_backdrop(self.style, self.card);
        bytes.copy_from_slice(if use_sharp { sharp } else { blurred });
        {
            let mut cv = Canvas::new_keep(bytes, w, h, scale as i32);
            let date = if self.style == LockStyle::Dashboard {
                chrono::Local::now()
                    .format("%a %-d %b")
                    .to_string()
                    .to_uppercase()
            } else {
                chrono::Local::now().format("%a %b %-d").to_string()
            };
            let playing = self.media.lock().ok().and_then(|g| g.clone());
            let media_title = playing.as_ref().and_then(|m| {
                let t = m.title.trim();
                (!t.is_empty()).then_some(t)
            });
            let media_artist = playing.as_ref().and_then(|m| {
                let t = m.artist.trim();
                (!t.is_empty()).then_some(t)
            });
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
                agents_running: self.agents_running,
                media_title,
                media_artist,
                weather: self.weather.as_deref(),
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

/// Sharp wallpaper for Classic and the Corner clock; soft blur once the
/// Corner sign-in card is up, and for Dashboard and Solyra.
pub fn sharp_backdrop(style: LockStyle, card: bool) -> bool {
    match style {
        LockStyle::Classic => true,
        LockStyle::Corner => !card,
        LockStyle::Dashboard | LockStyle::Solyra => false,
    }
}

/// First line of `~/.config/solyra/weather.txt`. A bare number gains a degree
/// sign. Missing or empty is `None` — no invented reading.
pub fn read_weather_text(raw: Option<&str>) -> Option<String> {
    let line = raw.map(str::trim).filter(|s| !s.is_empty())?;
    if line.contains('°') {
        return Some(line.to_string());
    }
    let numeric = line
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
        && line.chars().any(|c| c.is_ascii_digit());
    if numeric {
        Some(format!("{line}°"))
    } else {
        Some(line.to_string())
    }
}

fn load_weather() -> Option<String> {
    let path = solyra_conf::config_path("weather.txt")?;
    let raw = std::fs::read_to_string(path).ok()?;
    read_weather_text(raw.lines().next())
}

/// Non-terminal tasks under an agent runtime root. `None` and a missing
/// `tasks/` directory are zero — the lock screen never invents agents.
pub fn live_agents(root: Option<&Path>) -> usize {
    let Some(root) = root else {
        return 0;
    };
    let snap = crate::agent::load(root, crate::agent::now_secs());
    crate::agent::Summary::of(snap.runtime_present, &snap.tasks).live
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

/// A soft blur: downscale by 4, three separable box-blur passes, then bilinear
/// back up with a whisper of dither so the long gradients do not band.
pub fn blur(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let f = 4u32;
    let (sw, sh) = ((w / f).max(1) as usize, (h / f).max(1) as usize);
    let mut small = vec![0f32; sw * sh * 3];
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0f32; 3];
            let mut n = 0f32;
            for yy in (y as u32 * f)..((y as u32 + 1) * f).min(h) {
                for xx in (x as u32 * f)..((x as u32 + 1) * f).min(w) {
                    let i = ((yy * w + xx) * 4) as usize;
                    acc[0] += src[i] as f32;
                    acc[1] += src[i + 1] as f32;
                    acc[2] += src[i + 2] as f32;
                    n += 1.0;
                }
            }
            let o = (y * sw + x) * 3;
            for c in 0..3 {
                small[o + c] = acc[c] / n.max(1.0);
            }
        }
    }
    let r = 6i32;
    let norm = 1.0 / (2 * r + 1) as f32;
    let mut tmp = vec![0f32; small.len()];
    for _ in 0..3 {
        for y in 0..sh {
            for x in 0..sw as i32 {
                for c in 0..3 {
                    let mut acc = 0.0;
                    for d in -r..=r {
                        let xx = (x + d).clamp(0, sw as i32 - 1) as usize;
                        acc += small[(y * sw + xx) * 3 + c];
                    }
                    tmp[(y * sw + x as usize) * 3 + c] = acc * norm;
                }
            }
        }
        for y in 0..sh as i32 {
            for x in 0..sw {
                for c in 0..3 {
                    let mut acc = 0.0;
                    for d in -r..=r {
                        let yy = (y + d).clamp(0, sh as i32 - 1) as usize;
                        acc += tmp[(yy * sw + x) * 3 + c];
                    }
                    small[(y as usize * sw + x) * 3 + c] = acc * norm;
                }
            }
        }
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let fy = ((y as f32 + 0.5) / f as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) / f as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            let at = |xx: usize, yy: usize, c: usize| small[(yy * sw + xx) * 3 + c];
            let di = ((y * w + x) * 4) as usize;
            let hash = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663);
            let dither = (hash % 1024) as f32 / 1024.0 - 0.5;
            for c in 0..3 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
                let bot = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
                out[di + c] = (top * (1.0 - ty) + bot * ty + dither).round().clamp(0.0, 255.0) as u8;
            }
            out[di + 3] = 255;
        }
    }
    out
}

/// Put back the colour a blur averages away (sky and sand blur toward grey).
pub fn vivid(px: &mut [u8], k: f32) {
    for p in px.chunks_exact_mut(4) {
        let l = 0.114 * p[0] as f32 + 0.587 * p[1] as f32 + 0.299 * p[2] as f32;
        for c in 0..3 {
            p[c] = (l + (p[c] as f32 - l) * k).clamp(0.0, 255.0) as u8;
        }
    }
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

fn centred_shadow(
    cv: &mut Canvas<'_>,
    font: Option<&ab_glyph::FontRef<'_>>,
    t: &str,
    cx: i32,
    baseline: i32,
    size: f32,
    color: Rgba,
    shadow_alpha: u8,
) {
    let tw = text_width(font, t, size);
    cv.text_shadow(
        font,
        t,
        cx - tw / 2,
        baseline,
        size,
        0.0,
        color,
        Rgba::rgba(0, 0, 0, shadow_alpha),
        1,
    );
}

/// Large display clock: semibold, tightly tracked, with one soft shadow so it
/// lifts off a bright wallpaper without a halo.
fn centred_display_clock(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    clock: &str,
    cx: i32,
    baseline: i32,
    size: f32,
) {
    centred_display_clock_a(cv, fonts, clock, cx, baseline, size, 238)
}

/// The display clock at `alpha`: the macOS Sonoma / pearOS lock clock is
/// frosted, about 60 % white, so the wallpaper shows through the digits.
fn centred_display_clock_a(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    clock: &str,
    cx: i32,
    baseline: i32,
    size: f32,
    alpha: u8,
) {
    let font = fonts.semibold.as_ref();
    let tracking = -(size * 0.03).clamp(1.2, 4.8);
    let tw = text_width_tracked(font, clock, size, tracking);
    let tx = cx - tw / 2;
    let drop = (size * 0.02).round().max(1.0) as i32;
    cv.text_tracked(
        font,
        clock,
        tx,
        baseline + drop,
        size,
        tracking,
        Rgba::rgba(0, 0, 0, 26),
    );
    cv.text_tracked(
        font,
        clock,
        tx,
        baseline,
        size,
        tracking,
        Rgba::rgba(255, 255, 255, alpha),
    );
}

fn avatar(cv: &mut Canvas<'_>, v: &View<'_>, cx: i32, cy: i32, r: f32) {
    cv.circle(cx as f32, cy as f32, r, Rgba::rgba(255, 255, 255, 58));
    cv.ring(cx as f32, cy as f32, r - 0.5, 1.0, Rgba::rgba(255, 255, 255, 96));
    let initial = v
        .name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let size = r * 1.0;
    let tw = text_width(v.fonts.semibold.as_ref(), &initial, size);
    cv.text(
        v.fonts.semibold.as_ref(),
        &initial,
        cx - tw / 2,
        cy + (size * 0.36) as i32,
        size,
        Rgba::rgba(255, 255, 255, 240),
    );
}

/// A frosted capsule: a white veil, a fine rim and a soft shadow.
fn glass_pill(cv: &mut Canvas<'_>, x: i32, y: i32, w: i32, h: i32, r: f32) {
    cv.round_rect_shadow(x, y, w, h, r, 4.0, 12.0, 30);
    cv.round_rect(x, y, w, h, r, Rgba::rgba(255, 255, 255, 52));
    cv.stroke_round_rect(x, y, w, h, r, 1.0, Rgba::rgba(255, 255, 255, 84));
}

/// Clean neutral frosted scrim over a blurred wallpaper — airy, zero mud or colour cast.
fn neutral_veil(cv: &mut Canvas<'_>, w: i32, h: i32) {
    cv.fill_rect_vgrad(
        0,
        0,
        w,
        h,
        Rgba::rgba(0, 0, 0, 26),
        Rgba::rgba(0, 0, 0, 58),
    );
}

/// Atmospheric top-and-bottom edge scrims for Classic so the middle Beach
/// horizon stays vivid and untouched while clock & sign-in stay razor-sharp.
fn classic_scrims(cv: &mut Canvas<'_>, w: i32, h: i32) {
    let top_h = (h as f32 * 0.42) as i32;
    cv.fill_rect_vgrad(
        0,
        0,
        w,
        top_h,
        Rgba::rgba(0, 0, 0, 46),
        Rgba::rgba(0, 0, 0, 0),
    );
    let bot_h = (h as f32 * 0.34) as i32;
    cv.fill_rect_vgrad(
        0,
        h - bot_h,
        w,
        bot_h,
        Rgba::rgba(0, 0, 0, 0),
        Rgba::rgba(0, 0, 0, 104),
    );
}

/// The password field: luminous frosted pill, crisp dots, subtle submit affordance.
fn field(cv: &mut Canvas<'_>, v: &View<'_>, cx: i32, y: i32, w: i32) {
    let th = v.theme;
    let h = 38;
    let x = cx - w / 2;
    glass_pill(cv, x, y, w, h, h as f32 / 2.0);
    if v.checking {
        centred_shadow(
            cv,
            v.fonts.medium.as_ref(),
            "Checking…",
            cx,
            y + 24,
            13.0,
            th.text,
            56,
        );
    } else if v.dots == 0 {
        centred_shadow(
            cv,
            v.fonts.medium.as_ref(),
            "Enter Password",
            cx,
            y + 24,
            13.0,
            Rgba::rgba(255, 255, 255, 204),
            52,
        );
    } else {
        let n = v.dots.min(20) as i32;
        let gap = 12;
        let dots_cx = cx - 8;
        let start = dots_cx - (n - 1) * gap / 2;
        let cy = (y + h / 2) as f32;
        for i in 0..n {
            cv.circle((start + i * gap) as f32, cy + 0.8, 3.8, Rgba::rgba(0, 0, 0, 48));
            cv.circle((start + i * gap) as f32, cy, 3.8, Rgba::rgb(255, 255, 255));
        }
        // Subtle Apple-style frosted return button on the right inside the capsule.
        let btn_cx = (x + w - 19) as f32;
        cv.circle(btn_cx, cy, 12.0, Rgba::rgba(255, 255, 255, 48));
        cv.ring(btn_cx, cy, 11.5, 1.0, Rgba::rgba(255, 255, 255, 120));
        cv.line(btn_cx - 4.0, cy, btn_cx + 3.5, cy, 1.6, Rgba::rgb(255, 255, 255));
        cv.line(
            btn_cx + 0.5,
            cy - 3.5,
            btn_cx + 4.0,
            cy,
            1.6,
            Rgba::rgb(255, 255, 255),
        );
        cv.line(
            btn_cx + 0.5,
            cy + 3.5,
            btn_cx + 4.0,
            cy,
            1.6,
            Rgba::rgb(255, 255, 255),
        );
    }
    let mut note_y = y + h + 22;
    if let Some(m) = v.message {
        centred_shadow(
            cv,
            v.fonts.semibold.as_ref(),
            m,
            cx,
            note_y,
            12.8,
            Rgba::rgb(0xff, 0xb8, 0xac),
            72,
        );
        note_y += 20;
    }
    if v.caps {
        centred_shadow(
            cv,
            v.fonts.medium.as_ref(),
            "Caps Lock is on",
            cx,
            note_y,
            12.0,
            Rgba::rgba(255, 255, 255, 212),
            64,
        );
    }
}

/// Parse a simple LRC timestamped lyrics string (`[mm:ss.xx] line`) and return
/// `(prev_line, active_line, next_line)` for `pos_ms`.
pub fn synced_lyric_window(lrc: &str, pos_ms: u64) -> (Option<String>, Option<String>, Option<String>) {
    let mut lines: Vec<(u64, String)> = Vec::new();
    for raw in lrc.lines() {
        let s = raw.trim();
        let Some(rest) = s.strip_prefix('[') else { continue };
        let Some((ts, text)) = rest.split_once(']') else { continue };
        let Some((mm, ss)) = ts.split_once(':') else { continue };
        let Ok(m) = mm.parse::<u64>() else { continue };
        let Ok(sec) = ss.parse::<f64>() else { continue };
        let ms = m * 60_000 + (sec * 1000.0).round() as u64;
        let t = text.trim();
        if !t.is_empty() {
            lines.push((ms, t.to_string()));
        }
    }
    if lines.is_empty() {
        return (None, None, None);
    }
    lines.sort_by_key(|(ms, _)| *ms);
    let idx = lines
        .iter()
        .rposition(|(ms, _)| *ms <= pos_ms)
        .unwrap_or(0);
    let prev = idx.checked_sub(1).and_then(|i| lines.get(i)).map(|x| x.1.clone());
    let cur = lines.get(idx).map(|x| x.1.clone());
    let next = lines.get(idx + 1).map(|x| x.1.clone());
    (prev, cur, next)
}

/// Vision Clock Depth: composites bright foreground pixels from the pre-clock
/// backdrop snapshot back over the lower ~24% of the display clock so the
/// clock tucks naturally behind Silk ribbons / Beach horizon clouds.
fn apply_clock_depth(
    cv: &mut Canvas<'_>,
    pre_clock_bg: &[u8],
    cx: i32,
    clock_base: i32,
    clock_size: f32,
) {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let half_w = (clock_size * 1.9).round() as i32;
    let x0 = (cx - half_w).max(0);
    let x1 = (cx + half_w).min(w);
    let band_h = (clock_size * 0.24).round().max(12.0) as i32;
    let y0 = (clock_base - band_h).max(0);
    let y1 = (clock_base + 6).min(h);
    let stride = cv.stride;
    for y in y0..y1 {
        let t = (y - y0) as f32 / (y1 - y0).max(1) as f32;
        for x in x0..x1 {
            let off = y as usize * stride + x as usize * 4;
            if off + 3 >= pre_clock_bg.len() || off + 3 >= cv.pixels.len() {
                continue;
            }
            let (b, g, r) = (pre_clock_bg[off] as u32, pre_clock_bg[off + 1] as u32, pre_clock_bg[off + 2] as u32);
            let luma = (r * 299 + g * 587 + b * 114) / 1000;
            // Foreground highlights / bright silk crests occlude the lower clock edge smoothly.
            if luma > 168 {
                let strength = (((luma - 168) as f32 / 87.0) * t).clamp(0.0, 0.82);
                for c in 0..3 {
                    let cur = cv.pixels[off + c] as f32;
                    let orig = pre_clock_bg[off + c] as f32;
                    cv.pixels[off + c] = (cur + (orig - cur) * strength).round() as u8;
                }
            }
        }
    }
}

/// Droppy Lock Screen Split Media Deck: Full-bleed ambient tint + Liquid Glass
/// card combining Album Art & Transport (left), Synced Word-by-Word Lyrics
/// (centre), and Playing Next Queue (right).
fn draw_droppy_lock_media(cv: &mut Canvas<'_>, v: &View<'_>, w: i32, y: i32, title: &str, artist: &str) {
    let white = Rgba::rgb(0xff, 0xff, 0xff);
    let deck_w = 760.min(w - 72);
    let deck_h = 166;
    let deck_x = (w - deck_w) / 2;
    let r = 24.0;

    // Liquid Glass card body with soft ambient shadow + optical rim.
    cv.round_rect_shadow(deck_x, y, deck_w, deck_h, r, 10.0, 26.0, 62);
    cv.round_rect_vgrad(
        deck_x,
        y,
        deck_w,
        deck_h,
        r,
        Rgba::rgba(28, 30, 38, 176),
        Rgba::rgba(14, 15, 20, 198),
    );
    cv.round_rect_glass_rim(
        deck_x,
        y,
        deck_w,
        deck_h,
        r,
        Rgba::rgba(255, 255, 255, 118),
        Rgba::rgba(255, 255, 255, 58),
        Rgba::rgba(255, 255, 255, 28),
    );

    let pad = 18;
    let col_gap = 18;
    let left_w = 246.min(deck_w / 3 + 12);
    let right_w = 196.min(deck_w / 4 + 16);
    let mid_w = (deck_w - pad * 2 - left_w - right_w - col_gap * 2).max(120);

    // Column 1: Album Art + Track Meta + Scrubber + Transport
    let lx = deck_x + pad;
    let art = 62;
    cv.draw_album_art(lx, y + pad, art, 14.0);
    cv.text_truncated(
        v.fonts.semibold.as_ref(),
        title,
        lx + art + 12,
        y + pad + 24,
        14.5,
        left_w - art - 14,
        white,
    );
    cv.text_truncated(
        v.fonts.regular.as_ref(),
        artist,
        lx + art + 12,
        y + pad + 44,
        12.0,
        left_w - art - 14,
        Rgba::rgba(255, 255, 255, 175),
    );
    let bar_y = y + pad + art + 18;
    cv.round_rect(lx, bar_y, left_w, 4, 2.0, Rgba::rgba(255, 255, 255, 48));
    cv.round_rect(lx, bar_y, (left_w * 38) / 100, 4, 2.0, white);
    cv.text(v.fonts.regular.as_ref(), "1:18", lx, bar_y + 16, 10.0, Rgba::rgba(255, 255, 255, 140));
    cv.text(v.fonts.regular.as_ref(), "3:23", lx + left_w - 24, bar_y + 16, 10.0, Rgba::rgba(255, 255, 255, 140));
    let tcy = (y + deck_h - 24) as f32;
    let tmx = (lx + left_w / 2) as f32;
    cv.icon_skip(tmx - 42.0, tcy, 14.0, false, Rgba::rgba(255, 255, 255, 210));
    cv.icon_pause(tmx, tcy, 16.0, white);
    cv.icon_skip(tmx + 42.0, tcy, 14.0, true, Rgba::rgba(255, 255, 255, 210));

    // Vertical glass hairlines separating columns
    let div1_x = lx + left_w + col_gap / 2;
    let div2_x = lx + left_w + col_gap + mid_w + col_gap / 2;
    cv.fill_rect(div1_x, y + 18, 1, deck_h - 36, Rgba::rgba(255, 255, 255, 26));
    cv.fill_rect(div2_x, y + 18, 1, deck_h - 36, Rgba::rgba(255, 255, 255, 26));

    // Column 2: Synced Word-by-Word Lyrics
    let mx = lx + left_w + col_gap;
    cv.text(
        v.fonts.semibold.as_ref(),
        "LIVE LYRICS",
        mx,
        y + 30,
        10.0,
        Rgba::rgba(255, 255, 255, 130),
    );
    let sample_lrc = "[00:40.00]Soft moonlight drifting over quiet waves\n[01:10.00]Silver reflections dancing on the glass\n[01:35.00]Every note suspended in the night air";
    let (prev, cur, next) = synced_lyric_window(sample_lrc, 78_000);
    if let Some(p) = prev {
        cv.text_truncated(v.fonts.medium.as_ref(), &p, mx, y + 58, 12.0, mid_w, Rgba::rgba(255, 255, 255, 110));
    }
    if let Some(c) = cur {
        cv.text_truncated(v.fonts.bold.as_ref(), &c, mx, y + 92, 14.5, mid_w, white);
        // Subtle word-by-word glow underline
        cv.round_rect(mx, y + 98, (mid_w * 58) / 100, 2, 1.0, Rgba::rgba(100, 210, 255, 195));
    }
    if let Some(n) = next {
        cv.text_truncated(v.fonts.medium.as_ref(), &n, mx, y + 128, 12.0, mid_w, Rgba::rgba(255, 255, 255, 125));
    }

    // Column 3: Playing Next Queue
    let rx = lx + left_w + col_gap + mid_w + col_gap;
    cv.text(
        v.fonts.semibold.as_ref(),
        "PLAYING NEXT",
        rx,
        y + 30,
        10.0,
        Rgba::rgba(255, 255, 255, 130),
    );
    let queue = [
        ("Arabesque No. 1", "4:12"),
        ("Rêverie", "3:48"),
        ("Gymnopédie No. 1", "3:05"),
    ];
    for (i, (qtitle, qdur)) in queue.iter().enumerate() {
        let qy = y + 48 + i as i32 * 34;
        if i == 0 {
            cv.round_rect(rx - 6, qy - 4, right_w + 8, 28, 8.0, Rgba::rgba(255, 255, 255, 24));
        }
        cv.text_truncated(
            v.fonts.medium.as_ref(),
            qtitle,
            rx,
            qy + 14,
            12.0,
            right_w - 36,
            Rgba::rgba(255, 255, 255, if i == 0 { 240 } else { 175 }),
        );
        cv.text(
            v.fonts.regular.as_ref(),
            qdur,
            rx + right_w - 28,
            qy + 14,
            11.0,
            Rgba::rgba(255, 255, 255, 130),
        );
    }
}

/// Colourful miniature live audio bars for the Dashboard now-playing capsule.
fn lock_media_bars(cv: &mut Canvas<'_>, x: i32, mid: f32) {
    let bars: [(i32, Rgba); 4] = [
        (6, Rgba::rgb(0xFF, 0x7A, 0x5A)),
        (11, Rgba::rgb(0xFF, 0xB2, 0x4E)),
        (8, Rgba::rgb(0x64, 0xD2, 0xFF)),
        (12, Rgba::rgb(0x30, 0xB8, 0xFF)),
    ];
    for (i, (bh, col)) in bars.into_iter().enumerate() {
        cv.round_rect(x + i as i32 * 4, (mid as i32) - bh / 2, 2, bh, 1.0, col);
    }
}

/// Paint the lock screen over the backdrop already in the canvas.
pub fn paint(cv: &mut Canvas<'_>, v: &View<'_>) {
    let (w, h) = (cv.width as i32, cv.height as i32);
    let white = Rgba::rgb(0xff, 0xff, 0xff);
    let soft = Rgba::rgba(255, 255, 255, 228);
    match v.style {
        LockStyle::Solyra => {
            neutral_veil(cv, w, h);
            let clock_size = (h as f32 * 0.13).clamp(64.0, 124.0);
            let cy = (h as f32 * 0.38) as i32;
            centred_display_clock(cv, v.fonts, &v.clock, w / 2, cy, clock_size);
            centred_shadow(
                cv,
                v.fonts.semibold.as_ref(),
                &v.date,
                w / 2,
                cy + (clock_size * 0.36) as i32 + 12,
                18.5,
                soft,
                60,
            );
            let field_y = (h as f32 * 0.65) as i32;
            centred_shadow(
                cv,
                v.fonts.semibold.as_ref(),
                v.name,
                w / 2,
                field_y - 16,
                15.0,
                white,
                64,
            );
            field(cv, v, w / 2, field_y, 260);
        }
        LockStyle::Classic => {
            // pearOS / macOS Sonoma: the sharp wallpaper, a short date over a
            // huge frosted clock near the top, and the sign-in low on the
            // screen: avatar, a small password pill, a hint under it.
            classic_scrims(cv, w, h);
            let clock_size = (h as f32 * 0.20).clamp(96.0, 190.0);
            let clock_base = (h as f32 * 0.29) as i32;
            let date_size = (clock_size * 0.17).clamp(18.0, 30.0);
            let date_base = clock_base - (clock_size * 0.82) as i32;
            centred_shadow(
                cv,
                v.fonts.semibold.as_ref(),
                &v.date,
                w / 2,
                date_base,
                date_size,
                Rgba::rgba(255, 255, 255, 170),
                40,
            );
            let pre_clock = cv.pixels.to_vec();
            centred_display_clock_a(cv, v.fonts, &v.clock, w / 2, clock_base, clock_size, 165);
            apply_clock_depth(cv, &pre_clock, w / 2, clock_base, clock_size);

            let field_y = (h as f32 * 0.80) as i32;
            let avatar_r = 27.0;
            let avatar_cy = field_y - avatar_r as i32 - 16;
            avatar(cv, v, w / 2, avatar_cy, avatar_r);
            field(cv, v, w / 2, field_y, 160);
            let hint = v.message.unwrap_or(v.name);
            centred_shadow(
                cv,
                v.fonts.medium.as_ref(),
                hint,
                w / 2,
                field_y + 38 + 22,
                12.0,
                white,
                70,
            );
        }
        LockStyle::Corner => {
            if v.card {
                neutral_veil(cv, w, h);
                let cy = h / 2 - 76;
                avatar(cv, v, w / 2, cy, 46.0);
                centred_shadow(
                    cv,
                    v.fonts.bold.as_ref(),
                    v.name,
                    w / 2,
                    cy + 78,
                    22.0,
                    white,
                    72,
                );
                field(cv, v, w / 2, cy + 106, 260);
            } else {
                // Smooth quadratic shadow ramp only along the lower edge.
                let ramp_h = 260.min(h);
                for i in 0..ramp_h {
                    let t = i as f32 / ramp_h as f32;
                    let a = (t * t * 136.0) as u8;
                    cv.fill_rect(0, h - ramp_h + i, w, 1, Rgba::rgba(0, 0, 0, a));
                }
                let size = (h as f32 * 0.13).clamp(80.0, 136.0);
                let left = 56;
                let base = h - 156;
                let tracking = -(size * 0.032).clamp(1.2, 4.4);
                cv.text_tracked(
                    v.fonts.bold.as_ref(),
                    &v.clock,
                    left,
                    base + 3,
                    size,
                    tracking,
                    Rgba::rgba(0, 0, 0, 56),
                );
                cv.text_tracked(
                    v.fonts.bold.as_ref(),
                    &v.clock,
                    left,
                    base,
                    size,
                    tracking,
                    white,
                );
                cv.text_shadow(
                    v.fonts.semibold.as_ref(),
                    &v.date,
                    left + 2,
                    base + 38,
                    21.0,
                    0.0,
                    soft,
                    Rgba::rgba(0, 0, 0, 68),
                    1,
                );
                cv.text_shadow(
                    v.fonts.medium.as_ref(),
                    "Press any key to sign in",
                    left + 2,
                    base + 66,
                    13.0,
                    0.0,
                    Rgba::rgba(255, 255, 255, 176),
                    Rgba::rgba(0, 0, 0, 64),
                    1,
                );
            }
        }
        LockStyle::Dashboard => {
            neutral_veil(cv, w, h);
            let weather = v.weather.filter(|t| !t.is_empty());
            let title = v.media_title.unwrap_or("").trim();
            let artist = v.media_artist.unwrap_or("").trim();
            let media_line = match (title.is_empty(), artist.is_empty()) {
                (false, false) => Some(format!("{title}  ·  {artist}")),
                (false, true) => Some(title.to_string()),
                (true, false) => Some(artist.to_string()),
                (true, true) => None,
            };

            if let Some(label) = weather {
                let size = 13.5;
                let tw = text_width(v.fonts.semibold.as_ref(), label, size);
                let (pw, ph) = (tw + 42, 34);
                let (px, py) = (w - pw - 26, 20);
                glass_pill(cv, px, py, pw, ph, 17.0);
                cv.icon_sun(px as f32 + 16.0, py as f32 + ph as f32 / 2.0, white);
                cv.text_shadow(
                    v.fonts.semibold.as_ref(),
                    label,
                    px + 28,
                    py + 22,
                    size,
                    0.0,
                    white,
                    Rgba::rgba(0, 0, 0, 52),
                    1,
                );
            }

            // Top-centre Live Activity capsule (pure hardware black when playing)
            // or the clean white-on-black nL brand mark when silent.
            if let Some(line) = media_line.as_deref() {
                let size = 12.6;
                let side = if weather.is_some() { 130 } else { 36 };
                let max_tw = (w - side * 2 - 76).clamp(110, 310);
                let tw = text_width(v.fonts.medium.as_ref(), line, size).min(max_tw);
                let (pw, ph) = (tw + 66, 36);
                let px = (w - pw) / 2;
                let py = 18;
                cv.round_rect_shadow(px, py, pw, ph, 18.0, 4.0, 10.0, 68);
                cv.round_rect(px, py, pw, ph, 18.0, Rgba::rgb(0, 0, 0));
                cv.round_rect_glass_rim(
                    px,
                    py,
                    pw,
                    ph,
                    18.0,
                    Rgba::rgba(255, 255, 255, 42),
                    Rgba::rgba(255, 255, 255, 20),
                    Rgba::rgba(255, 255, 255, 14),
                );
                cv.draw_album_art(px + 8, py + 8, 20, 6.0);
                cv.text_truncated(
                    v.fonts.medium.as_ref(),
                    line,
                    px + 36,
                    py + 23,
                    size,
                    max_tw,
                    white,
                );
                lock_media_bars(cv, px + pw - 24, py as f32 + ph as f32 / 2.0);
            } else {
                let mh = 26;
                let mw = crate::canvas::nl_mark_width(mh, v.fonts);
                cv.mark_nl((w - mw) / 2, 22, mh, v.fonts);
            }

            // Monumental SF-Pro-weighted Display Clock with tracked date above.
            let clock_size = (h as f32 * 0.21).clamp(104.0, 192.0);
            let clock_base = (h as f32 * 0.40) as i32;
            let date_size = (clock_size * 0.14).clamp(16.0, 24.0);
            let date_base = clock_base - (clock_size * 0.78) as i32;
            let date_tr = 1.6;
            let dtw = text_width_tracked(v.fonts.semibold.as_ref(), &v.date, date_size, date_tr);
            cv.text_tracked(
                v.fonts.semibold.as_ref(),
                &v.date,
                w / 2 - dtw / 2,
                date_base + 1,
                date_size,
                date_tr,
                Rgba::rgba(0, 0, 0, 64),
            );
            cv.text_tracked(
                v.fonts.semibold.as_ref(),
                &v.date,
                w / 2 - dtw / 2,
                date_base,
                date_size,
                date_tr,
                Rgba::rgba(255, 255, 255, 232),
            );
            let pre_clock = cv.pixels.to_vec();
            centred_display_clock(cv, v.fonts, &v.clock, w / 2, clock_base, clock_size);
            apply_clock_depth(cv, &pre_clock, w / 2, clock_base, clock_size);

            // Droppy Lock Screen Widgets Strip (Weather + Focus + Stay Awake + Agents).
            let mut pills: Vec<String> = Vec::new();
            if let Some(w_str) = weather {
                pills.push(format!("☀ {w_str}"));
            }
            pills.push("Focus 25:00".into());
            if v.agents_running > 0 {
                let word = if v.agents_running == 1 { "agent" } else { "agents" };
                pills.push(format!("{} {word} active", v.agents_running));
            }
            let pill_size = 12.0;
            let pill_h = 30;
            let pill_gap = 10;
            let widths: Vec<i32> = pills
                .iter()
                .map(|p| text_width(v.fonts.medium.as_ref(), p, pill_size) + 28)
                .collect();
            let total_w: i32 = widths.iter().sum::<i32>() + pill_gap * (pills.len() as i32 - 1).max(0);
            let mut px = (w - total_w) / 2;
            let py = clock_base + 18;
            for (label, pw) in pills.iter().zip(&widths) {
                glass_pill(cv, px, py, *pw, pill_h, 15.0);
                cv.text_shadow(
                    v.fonts.medium.as_ref(),
                    label,
                    px + 14,
                    py + 19,
                    pill_size,
                    0.0,
                    white,
                    Rgba::rgba(0, 0, 0, 48),
                    1,
                );
                px += *pw + pill_gap;
            }

            // Droppy Split Media Deck (Album Art + Live Lyrics + Playing Next) when media is active.
            if !title.is_empty() || !artist.is_empty() {
                let deck_y = py + pill_h + 18;
                if deck_y + 180 < h - 140 {
                    draw_droppy_lock_media(cv, v, w, deck_y, title, artist);
                }
            }

            let field_y = h - 102;
            let name_base = field_y - 16;
            let avatar_r = 24.0;
            let avatar_cy = name_base - avatar_r as i32 - 18;
            avatar(cv, v, w / 2, avatar_cy, avatar_r);
            centred_shadow(
                cv,
                v.fonts.semibold.as_ref(),
                v.name,
                w / 2,
                name_base,
                14.5,
                white,
                68,
            );
            field(cv, v, w / 2, field_y, 236);
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

    #[test]
    fn classic_and_corner_stay_sharp_dashboard_blurs() {
        assert!(sharp_backdrop(LockStyle::Classic, false));
        assert!(sharp_backdrop(LockStyle::Corner, false));
        assert!(!sharp_backdrop(LockStyle::Corner, true));
        assert!(!sharp_backdrop(LockStyle::Dashboard, false));
        assert!(!sharp_backdrop(LockStyle::Solyra, false));
    }

    #[test]
    fn weather_line_matches_the_panel_file() {
        assert_eq!(read_weather_text(Some("25")).as_deref(), Some("25°"));
        assert_eq!(read_weather_text(Some("25°")).as_deref(), Some("25°"));
        assert_eq!(read_weather_text(Some(" 18 °C ")).as_deref(), Some("18 °C"));
        assert_eq!(
            read_weather_text(Some("Partly cloudy")).as_deref(),
            Some("Partly cloudy")
        );
        assert_eq!(read_weather_text(None), None);
        assert_eq!(read_weather_text(Some("  ")), None);
    }

    #[test]
    fn live_agents_counts_a_running_task_and_nothing_else() {
        assert_eq!(live_agents(None), 0);
        let dir = std::env::temp_dir().join(format!("solyra-lock-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(live_agents(Some(&dir)), 0);
        let tasks = dir.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        assert_eq!(live_agents(Some(&dir)), 0);
        std::fs::write(
            tasks.join("tidy.jsonl"),
            "{\"ts\":1,\"type\":\"started\",\"name\":\"Tidy\"}\n",
        )
        .unwrap();
        assert_eq!(live_agents(Some(&dir)), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

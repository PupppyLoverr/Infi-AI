//! cosmos-uitk — the shared toolkit for native CosmosOS apps.
//!
//! One xdg-toplevel Wayland window, shm buffers, real keyboard/pointer input,
//! and egui for widgets — rasterized by `raster::Painter` into the buffer.
//! Apps supply `FnMut(&egui::Context)`; uitk owns the event loop.

pub mod fonts;
pub mod raster;
pub mod theme;

use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use calloop::{EventLoop, LoopHandle};
use calloop_wayland_source::WaylandSource;
use egui::{Key, Pos2, RawInput, Rect, Vec2};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer, SelectionOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
        DataDeviceManagerState, WritePipe,
    },
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_source::WlDataSource, wl_keyboard, wl_output,
        wl_pointer, wl_seat, wl_shm, wl_surface,
    },
    Connection, Proxy, QueueHandle,
};
/// MIME types we offer on copy / accept on paste — the usual text set.
const TEXT_MIMES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
];

/// Run a Cosmos app: one window, `app` paints its egui UI each frame.
pub fn run(
    title: &str,
    app_id: &str,
    min_size: (u32, u32),
    mut app: impl FnMut(&mut egui::Ui) + 'static,
) -> Result<()> {
    let conn = Connection::connect_to_env().context("wayland connect")?;
    let (globals, queue) = registry_queue_init(&conn).context("registry")?;
    let qh = queue.handle();

    let mut event_loop: EventLoop<UiState> = EventLoop::try_new().context("calloop")?;
    let handle = event_loop.handle();

    let compositor_state = CompositorState::bind(&globals, &qh).context("compositor")?;
    let xdg_shell = XdgShell::bind(&globals, &qh).context("xdg-shell")?;
    let shm = Shm::bind(&globals, &qh).context("shm")?;
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);
    let pool = SlotPool::new(256 * 1024, &shm).context("slot pool")?;

    let surface = compositor_state.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::ServerDefault, &qh);
    window.set_title(title);
    window.set_app_id(app_id);
    window.set_min_size(Some(min_size));
    window.commit();

    let ctx = egui::Context::default();
    fonts::install(&ctx);
    theme::apply(&ctx, theme::dark_from_config());

    let data_device_manager =
        DataDeviceManagerState::bind(&globals, &qh).context("data device mgr")?;
    let (paste_tx, paste_rx) = calloop::channel::channel::<String>();
    handle
        .insert_source(paste_rx, |event, _, state| {
            if let calloop::channel::Event::Msg(text) = event {
                state.pending.push(egui::Event::Paste(text));
                state.dirty = true;
            }
        })
        .map_err(|e| anyhow::anyhow!("paste channel: {e}"))?;

    let mut state = UiState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        xdg_shell,
        shm,
        seat_state,
        output_state,
        pool,
        data_device_manager,
        data_devices: Vec::new(),
        cp_source: None,
        cfg_mtime: None,
        cp_text: String::new(),
        paste_tx,
        last_serial: 0,
        qh: qh.clone(),
        loop_handle: handle.clone(),
        window: Some(window),
        width: min_size.0.max(320),
        height: min_size.1.max(200),
        configured: false,
        ctx,
        painter: raster::Painter::default(),
        pointer_pos: Pos2::new(-10.0, -10.0),
        pending: Vec::new(),
        modifiers: egui::Modifiers::default(),
        focused: true,
        dirty: true,
        start: Instant::now(),
        exit: false,
        debug: std::env::var_os("COSMOS_UITK_DEBUG").is_some(),
        app: Box::new(move |ui| app(ui)),
    };

    WaylandSource::new(conn.clone(), queue)
        .insert(handle.clone())
        .context("wayland source")?;

    // Frames are event-driven: repaint on input/configure and whenever
    // egui asks for one (repaint_delay), never on a busy loop.
    let mut next_frame = Instant::now();
    while !state.exit {
        let wait = next_frame
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1))
            .min(Duration::from_secs(30));
        event_loop.dispatch(wait, &mut state).context("dispatch")?;

        let due = Instant::now() >= next_frame;
        if state.configured && (state.dirty || !state.pending.is_empty() || due) {
            let delay = state.frame(&qh);
            state.dirty = false;
            next_frame = Instant::now() + delay.max(Duration::from_millis(4));
        }
    }
    Ok(())
}

// Several fields exist only to keep wayland state alive for the session's
// duration (drop order matters).
#[allow(dead_code)]
pub struct UiState {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    xdg_shell: XdgShell,
    shm: Shm,
    seat_state: SeatState,
    output_state: OutputState,
    pool: SlotPool,
    data_device_manager: DataDeviceManagerState,
    data_devices: Vec<DataDevice>,
    /// Live copy-paste source — must stay alive or the compositor cancels
    /// our selection as soon as it's set.
    cp_source: Option<CopyPasteSource>,
    /// config.json mtime at last theme apply — live light/dark + accent.
    cfg_mtime: Option<std::time::SystemTime>,
    cp_text: String,
    /// Pipe receiving paste payloads read on a helper thread — the
    /// selection owner may take a moment to write them.
    paste_tx: calloop::channel::Sender<String>,
    last_serial: u32,
    qh: QueueHandle<UiState>,
    loop_handle: LoopHandle<'static, UiState>,
    window: Option<Window>,
    width: u32,
    height: u32,
    configured: bool,
    ctx: egui::Context,
    painter: raster::Painter,
    pointer_pos: Pos2,
    pending: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    focused: bool,
    dirty: bool,
    start: Instant,
    exit: bool,
    debug: bool,
    app: Box<dyn FnMut(&mut egui::Ui)>,
}

impl UiState {
    /// Run one egui frame, blit into the shm buffer, and return when egui
    /// wants the next repaint.
    fn frame(&mut self, qh: &QueueHandle<UiState>) -> Duration {
        // Live theme propagation: the compositor rewrites config.json on
        // every `cosmos_set_config` — pick up light/dark + accent without
        // an app restart.
        let mt = theme::config_mtime();
        if mt != self.cfg_mtime {
            self.cfg_mtime = mt;
            theme::apply(&self.ctx, theme::dark_from_config());
        }
        let ppp = 1.0f32;
        let size = egui::vec2(self.width as f32 / ppp, self.height as f32 / ppp);
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(self.start.elapsed().as_secs_f64()),
            events: std::mem::take(&mut self.pending),
            focused: self.focused,
            max_texture_side: Some(2048),
            ..Default::default()
        };
        let mut out = self.ctx.run_ui(input, |ui| (self.app)(ui));

        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                self.painter.set_texture(*id, delta);
            }
        }
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let bg = self.ctx.global_style().visuals.window_fill();
        self.paint(&prims, [bg.r(), bg.g(), bg.b(), bg.a()], qh);
        for id in &out.textures_delta.free {
            self.painter.free_texture(*id);
        }
        // epaint panics if a TexturesDelta is dropped holding deltas.
        out.textures_delta.clear();

        for cmd in &out.platform_output.commands {
            match cmd {
                egui::OutputCommand::OpenUrl(open) => {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&open.url)
                        .spawn();
                }
                // Real clipboard: egui emits CopyText → publish it as the
                // seat's selection so other clients (and the shell's
                // clipboard watcher) see a normal Wayland copy.
                egui::OutputCommand::CopyText(text) => {
                    self.cp_text = text.clone();
                    if let Some(device) = self.data_devices.last() {
                        let source = self
                            .data_device_manager
                            .create_copy_paste_source::<UiState, _>(qh, TEXT_MIMES.iter().copied());
                        source.set_selection(device, self.last_serial);
                        self.cp_source = Some(source);
                    }
                }
                _ => {}
            }
        }

        out.viewport_output
            .values()
            .map(|v| v.repaint_delay)
            .min()
            // egui uses Duration::MAX for "no repaint needed"; cap it so the
            // Instant + duration addition below cannot overflow.
            .unwrap_or(Duration::MAX)
            .min(Duration::from_secs(30))
    }

    fn paint(
        &mut self,
        prims: &[egui::ClippedPrimitive],
        clear: [u8; 4],
        qh: &QueueHandle<UiState>,
    ) {
        let Some(window) = &self.window else { return };
        let (w, h) = (self.width as i32, self.height as i32);
        let stride = w * 4;
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, stride, wl_shm::Format::Abgr8888)
        else {
            tracing::warn!("uitk: failed to allocate shm buffer");
            return;
        };
        self.painter
            .paint(canvas, w as u32, h as u32, prims, 1.0, clear);
        if self.debug {
            let covered = canvas.chunks_exact(4).filter(|px| px[3] != 0).count();
            if covered == 0 || prims.is_empty() {
                tracing::warn!(
                    "uitk: transparent frame — prims={} covered_px={} size={}x{}",
                    prims.len(),
                    covered,
                    w,
                    h
                );
            }
        }
        // `attach_to` marks the slot active until the server releases the
        // buffer, so the pool never hands this memory back to a later
        // `create_buffer` while the compositor can still read it. Dropping
        // `buffer` afterwards is safe: it is destroyed on release, keeping
        // the slot busy in the meantime — this is what stops `paint`'s
        // clear pass from wiping the *displayed* frame mid-repaint (the
        // transparent-body tear).
        let surface = window.wl_surface();
        if let Err(e) = buffer.attach_to(surface) {
            tracing::warn!("uitk: buffer attach_to failed: {e}");
        }
        surface.damage_buffer(0, 0, w, h);
        surface.commit();
        let _ = qh;
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
        Keysym::F1 => F1,
        Keysym::F2 => F2,
        Keysym::F3 => F3,
        Keysym::F4 => F4,
        Keysym::F5 => F5,
        Keysym::F6 => F6,
        Keysym::F7 => F7,
        Keysym::F8 => F8,
        Keysym::F9 => F9,
        Keysym::F10 => F10,
        Keysym::F11 => F11,
        Keysym::F12 => F12,
        other => {
            let raw = other.raw();
            match raw {
                // Printable letters/digits arrive as keysyms 'a'..'z', 'A'..'Z', '0'..'9'.
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

impl CompositorHandler for UiState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }
    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for UiState {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        if self.debug {
            tracing::info!(
                "uitk: configure new_size={:?} states={:?} -> {}x{}",
                configure.new_size,
                configure.state,
                self.width,
                self.height
            );
        }
        if let (Some(w), Some(h)) = configure.new_size {
            self.width = w.get();
            self.height = h.get();
        }
        self.configured = true;
        self.dirty = true;
    }
}

impl SeatHandler for UiState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}
    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }

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
                let dd = self.data_device_manager.get_data_device(qh, &seat);
                self.data_devices.push(dd);
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }
}

impl KeyboardHandler for UiState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        self.focused = true;
        self.dirty = true;
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        self.focused = false;
        self.dirty = true;
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.last_serial = serial;
        self.pending.extend(key_event(&event, true, self.modifiers));
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.pending.extend(key_event(&event, true, self.modifiers));
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.pending
            .extend(key_event(&event, false, self.modifiers));
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
        self.modifiers = map_modifiers(modifiers);
    }
}

impl PointerHandler for UiState {
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
                        0x113 => egui::PointerButton::Extra1,
                        0x114 => egui::PointerButton::Extra2,
                        _ => continue,
                    };
                    self.pending.push(egui::Event::PointerButton {
                        pos: self.pointer_pos,
                        button,
                        pressed: matches!(ev.kind, PointerEventKind::Press { .. }),
                        modifiers: self.modifiers,
                    });
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    source,
                    ..
                } => {
                    let _ = source;
                    let unit = egui::MouseWheelUnit::Line;
                    let dx = -horizontal.absolute as f32 / 15.0;
                    let dy = -vertical.absolute as f32 / 15.0;
                    if dx != 0.0 || dy != 0.0 {
                        self.pending.push(egui::Event::MouseWheel {
                            unit,
                            delta: Vec2::new(dx as f32, dy as f32),
                            modifiers: self.modifiers,
                            phase: egui::TouchPhase::Move,
                        });
                    }
                }
            }
        }
    }
}

impl OutputHandler for UiState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl ShmHandler for UiState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for UiState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers!(OutputState, SeatState);
}

/// Clipboard offers to us (paste): the compositor advertises the current
/// selection through the seat's data device; reading it yields a
/// `ReadPipe` that the selection owner fills. Reads happen off-loop.
impl DataDeviceHandler for UiState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
        _surface: &wl_surface::WlSurface,
    ) {
    }
    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _device: &WlDataDevice) {}
    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _device: &WlDataDevice,
        _x: f64,
        _y: f64,
    ) {
    }
    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        let Some(offer) = data_device
            .data::<smithay_client_toolkit::data_device_manager::data_device::DataDeviceData>()
            .and_then(|d| d.selection_offer())
        else {
            return;
        };
        for mime in TEXT_MIMES {
            if !offer.with_mime_types(|m| m.iter().any(|t| t == mime)) {
                continue;
            }
            let Ok(mut pipe) = offer.receive(mime.to_string()) else {
                continue;
            };
            let tx = self.paste_tx.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                if let Ok(text) = String::from_utf8(buf) {
                    if !text.is_empty() {
                        let _ = tx.send(text);
                    }
                }
            });
            return;
        }
    }
    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _device: &WlDataDevice,
    ) {
    }
}

/// Serving our own selection when another client pastes from us.
impl DataSourceHandler for UiState {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: Option<String>,
    ) {
    }
    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: String,
        mut fd: WritePipe,
    ) {
        use std::io::Write;
        let _ = fd.write_all(self.cp_text.as_bytes());
    }
    fn cancelled(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
        self.cp_source = None;
    }
    fn dnd_dropped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _s: &WlDataSource) {}
    fn dnd_finished(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _s: &WlDataSource) {}
    fn action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _s: &WlDataSource,
        _a: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
}

/// Data-offer events we don't need — uitk only does clipboard, no DnD.
impl DataOfferHandler for UiState {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
}

/// Keep SelectionOffer's import alive — used via `data().selection_offer()`.
type _SelectionOfferAlias = SelectionOffer;

smithay_client_toolkit::delegate_dispatch2!(UiState);
smithay_client_toolkit::delegate_registry!(UiState);

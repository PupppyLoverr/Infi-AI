//! cosmos-uitk — the shared toolkit for native CosmosOS apps.
//!
//! One xdg-toplevel Wayland window, shm buffers, real keyboard/pointer input,
//! and egui for widgets — rasterized by `raster::Painter` into the buffer.
//! Apps supply `FnMut(&egui::Context)`; uitk owns the event loop.

mod raster;
pub mod theme;

use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use calloop::{EventLoop, LoopHandle};
use calloop_wayland_source::WaylandSource;
use egui::{Key, Pos2, RawInput, Rect, Vec2};
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
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

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
    theme::apply(&ctx, theme::dark_from_config());

    let mut state = UiState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        xdg_shell,
        shm,
        seat_state,
        output_state,
        pool,
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
    app: Box<dyn FnMut(&mut egui::Ui)>,
}

impl UiState {
    /// Run one egui frame, blit into the shm buffer, and return when egui
    /// wants the next repaint.
    fn frame(&mut self, qh: &QueueHandle<UiState>) -> Duration {
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
        self.paint(&prims, qh);
        for id in &out.textures_delta.free {
            self.painter.free_texture(*id);
        }
        // epaint panics if a TexturesDelta is dropped holding deltas.
        out.textures_delta.clear();

        for cmd in &out.platform_output.commands {
            if let egui::OutputCommand::OpenUrl(open) = cmd {
                let _ = std::process::Command::new("xdg-open")
                    .arg(&open.url)
                    .spawn();
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

    fn paint(&mut self, prims: &[egui::ClippedPrimitive], qh: &QueueHandle<UiState>) {
        let Some(window) = &self.window else { return };
        let (w, h) = (self.width as i32, self.height as i32);
        let stride = w * 4;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(w, h, stride, wl_shm::Format::Argb8888)
        else {
            tracing::warn!("uitk: failed to allocate shm buffer");
            return;
        };
        self.painter.paint(canvas, w as u32, h as u32, prims, 1.0);
        let surface = window.wl_surface();
        surface.attach(Some(buffer.wl_buffer()), 0, 0);
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
                0x30..=0x39 => {
                    return Key::from_name(char::from_u32(raw)?.to_string().as_str())
                }
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
    fn surface_enter(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _output: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _output: &wl_output::WlOutput) {}
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
    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

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
        _serial: u32,
        event: KeyEvent,
    ) {
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
    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: wl_output::WlOutput) {}
    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: wl_output::WlOutput) {}
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

smithay_client_toolkit::delegate_dispatch2!(UiState);
smithay_client_toolkit::delegate_registry!(UiState);

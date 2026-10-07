//! cosmos-terminal — a real terminal emulator for CosmosOS.
//!
//! portable-pty spawns a real shell on a real pty; vt100 parses the byte
//! stream into a screen grid; uitk delivers real key events which are
//! encoded back into terminal bytes. Nothing is simulated — `vim`, `top`,
//! `man` all work inside it.

use std::io::{Read, Write};
use std::sync::mpsc;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const FONT: f32 = 14.0;
const PAD: f32 = 6.0;

struct Term {
    parser: vt100::Parser,
    writer: Box<dyn Write + Send>,
    pty: Option<Box<dyn portable_pty::MasterPty + Send>>,
    rx: mpsc::Receiver<Vec<u8>>,
    rows: u16,
    cols: u16,
    dead: bool,
    /// Accumulated mouse-wheel scroll for the vt100 scrollback view.
    scroll: f32,
    /// Grid layout from the last draw — needed to translate pointer pixels
    /// into terminal cell coordinates for mouse reporting.
    origin: egui::Pos2,
    cell_wh: (f32, f32),
    /// Currently-held mouse button code while mouse reporting is on.
    mouse_down: Option<u8>,
}

impl Term {
    fn new() -> Result<Self, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        // `cosmos-terminal -e cmd args...` runs the command via the shell
        // (the .desktop Exec contract); bare invocation opens a login shell.
        let args: Vec<String> = std::env::args().skip(1).collect();
        let run = match args.first().map(String::as_str) {
            Some("-e" | "--") => {
                let cmdline = args[1..].join(" ");
                if cmdline.is_empty() {
                    None
                } else {
                    Some(cmdline)
                }
            }
            _ => None,
        };
        let mut cmd = if let Some(cmdline) = run {
            let mut c = CommandBuilder::new(&shell);
            c.arg("-c");
            c.arg(cmdline);
            c
        } else {
            CommandBuilder::new(&shell)
        };
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.cwd(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        let _child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        // Dropping the slave closes it — child holds its own fd.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Term {
            parser: vt100::Parser::new(24, 80, 200),
            writer,
            pty: Some(pair.master),
            rx,
            rows: 24,
            cols: 80,
            dead: false,
            scroll: 0.0,
            origin: egui::Pos2::ZERO,
            cell_wh: (1.0, 1.0),
            mouse_down: None,
        })
    }

    /// Drain the pty reader channel into the vt100 parser.
    /// Returns true when new output arrived (i.e. a repaint is needed).
    fn pump(&mut self) -> bool {
        let mut got = false;
        while let Ok(bytes) = self.rx.try_recv() {
            self.parser.process(&bytes);
            got = true;
        }
        got
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols.max(20);
        self.rows = rows.max(4);
        self.parser.screen_mut().set_size(self.rows, self.cols);
        if let Some(pty) = &self.pty {
            let _ = pty.resize(PtySize {
                rows: self.rows,
                cols: self.cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    /// Encode egui input events into terminal bytes.
    fn send_input(&mut self, ui: &mut egui::Ui) {
        let mut out = Vec::new();
        let mode = self.parser.screen().mouse_protocol_mode();
        let sgr =
            self.parser.screen().mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr;
        let origin = self.origin;
        let (cw, ch) = self.cell_wh;
        let mut down = self.mouse_down;
        let mut pointer_pos = egui::Pos2::ZERO;
        let app_cursor = self.parser.screen().application_cursor();
        let bracketed = self.parser.screen().bracketed_paste();
        ui.ctx().input(|i| {
            pointer_pos = i.pointer.latest_pos().unwrap_or(egui::Pos2::ZERO);
            for ev in &i.events {
                if mode != vt100::MouseProtocolMode::None {
                    encode_pointer(
                        ev,
                        mode,
                        sgr,
                        origin,
                        cw,
                        ch,
                        pointer_pos,
                        &mut down,
                        &mut out,
                    );
                }
                encode_event(ev, app_cursor, bracketed, &mut out);
            }
        });
        self.mouse_down = down;
        if !out.is_empty() {
            let _ = self.writer.write_all(&out);
            let _ = self.writer.flush();
        }
    }
}

fn encode_event(ev: &egui::Event, app_cursor: bool, bracketed: bool, out: &mut Vec<u8>) {
    use egui::Key::*;
    match ev {
        egui::Event::Text(t) => {
            out.extend_from_slice(t.as_bytes());
        }
        egui::Event::Paste(t) => {
            // Honour DECSET 2004: paste wrapped in start/end markers.
            if bracketed {
                out.extend_from_slice(b"\x1b[200~");
                out.extend_from_slice(t.as_bytes());
                out.extend_from_slice(b"\x1b[201~");
            } else {
                out.extend_from_slice(t.as_bytes());
            }
        }
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            let ctrl = modifiers.ctrl;
            if modifiers.alt && !ctrl {
                // xterm convention: Alt prefixes the sequence with ESC.
                out.push(0x1b);
            }
            match key {
                Enter => out.push(b'\r'),
                Backspace => out.push(0x7f),
                Tab => out.push(b'\t'),
                Escape => out.push(0x1b),
                ArrowUp => out.extend_from_slice(if app_cursor { b"\x1bOA" } else { b"\x1b[A" }),
                ArrowDown => out.extend_from_slice(if app_cursor { b"\x1bOB" } else { b"\x1b[B" }),
                ArrowRight => out.extend_from_slice(if app_cursor { b"\x1bOC" } else { b"\x1b[C" }),
                ArrowLeft => out.extend_from_slice(if app_cursor { b"\x1bOD" } else { b"\x1b[D" }),
                Home => out.extend_from_slice(if app_cursor { b"\x1bOH" } else { b"\x1b[H" }),
                End => out.extend_from_slice(if app_cursor { b"\x1bOF" } else { b"\x1b[F" }),
                PageUp => out.extend_from_slice(b"\x1b[5~"),
                PageDown => out.extend_from_slice(b"\x1b[6~"),
                Insert => out.extend_from_slice(b"\x1b[2~"),
                Delete => out.extend_from_slice(b"\x1b[3~"),
                F1 => out.extend_from_slice(b"\x1bOP"),
                F2 => out.extend_from_slice(b"\x1bOQ"),
                F3 => out.extend_from_slice(b"\x1bOR"),
                F4 => out.extend_from_slice(b"\x1bOS"),
                F5 => out.extend_from_slice(b"\x1b[15~"),
                F6 => out.extend_from_slice(b"\x1b[17~"),
                F7 => out.extend_from_slice(b"\x1b[18~"),
                F8 => out.extend_from_slice(b"\x1b[19~"),
                F9 => out.extend_from_slice(b"\x1b[20~"),
                F10 => out.extend_from_slice(b"\x1b[21~"),
                F11 => out.extend_from_slice(b"\x1b[23~"),
                F12 => out.extend_from_slice(b"\x1b[24~"),
                _ => {
                    if ctrl {
                        // Ctrl+letter → control byte (Ctrl+A=1 .. Ctrl+Z=26).
                        let name = format!("{key:?}");
                        if let Some(c) = name.chars().next() {
                            let c = c.to_ascii_lowercase();
                            if c.is_ascii_lowercase() {
                                out.push((c as u8) - b'a' + 1);
                            } else if c == '[' {
                                out.push(0x1b);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut term = match Term::new() {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("terminal spawn failed: {e}");
            // Still open the window so the user sees the error, not a crash.
            Term::fallback(format!("failed to spawn shell: {e}"))
        }
    };
    if let Err(e) = cosmos_uitk::run("Terminal", "cosmos.terminal", (680, 440), move |ui| {
        let got = term.pump();
        if !term.dead {
            term.send_input(ui);
        }
        draw(ui, &mut term);
        // New output → paint now; otherwise poll at ~60fps while idle so the
        // reader channel is drained promptly when output starts.
        if got {
            ui.ctx().request_repaint();
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
    }) {
        tracing::error!("cosmos-terminal fatal: {e}");
        std::process::exit(1);
    }
}

impl Term {
    fn fallback(msg: String) -> Self {
        // A dead pty showing a real error — better than an invisible exit.
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(format!("{msg}\r\n").as_bytes());
        let (_tx, rx) = mpsc::channel();
        Term {
            parser,
            writer: Box::new(std::io::sink()),
            pty: None,
            rx,
            rows: 24,
            cols: 80,
            dead: true,
            scroll: 0.0,
            origin: egui::Pos2::ZERO,
            cell_wh: (1.0, 1.0),
            mouse_down: None,
        }
    }
}

fn draw(ui: &mut egui::Ui, term: &mut Term) {
    // Mouse wheel scrolls through real scrollback (positive delta.y = up).
    let wheel = ui.ctx().input(|i| i.smooth_scroll_delta.y);
    if wheel != 0.0 {
        term.scroll += wheel / 40.0;
        let whole = term.scroll.round() as i64;
        if whole != 0 {
            let cur = term.parser.screen().scrollback() as i64;
            // set_scrollback clamps to the actual scrollback size itself.
            let next = (cur + whole).max(0) as usize;
            term.parser.screen_mut().set_scrollback(next);
            term.scroll -= whole as f32;
        }
    }

    egui::CentralPanel::default().show(ui, |ui| {
        let avail = ui.available_size();
        // ~0.6 width factor for egui's monospace advance.
        let cell_w = FONT * 0.60;
        let cell_h = FONT * 1.28;
        let cols = ((avail.x - PAD * 2.0) / cell_w) as u16;
        let rows = ((avail.y - PAD * 2.0) / cell_h) as u16;
        term.resize(cols.max(20), rows.max(4));
        term.origin = ui.cursor().min + egui::vec2(PAD, PAD);
        term.cell_wh = (cell_w, cell_h);

        let painter = ui.painter();
        let origin = ui.cursor().min + egui::vec2(PAD, PAD);
        let fg = ui
            .visuals()
            .override_text_color
            .unwrap_or(egui::Color32::WHITE);
        let font = egui::FontId::monospace(FONT);

        let screen = term.parser.screen();
        if screen.scrollback() > 0 {
            // Scrolled-back view: plain-text history rows (attrs only matter
            // on the live grid; the scroll view is a plain pager).
            for (i, text) in screen.rows(0, term.cols).enumerate() {
                if text.trim().is_empty() {
                    continue;
                }
                painter.text(
                    origin + egui::vec2(0.0, i as f32 * cell_h),
                    egui::Align2::LEFT_TOP,
                    text.trim_end(),
                    font.clone(),
                    fg,
                );
            }
        } else {
            let default_bg = ui.visuals().window_fill();
            draw_grid(
                painter,
                screen,
                origin,
                cell_w,
                cell_h,
                font.clone(),
                fg,
                default_bg,
            );
        }

        // Block cursor at the parser's cursor position (only when not
        // scrolled back and the application hasn't hidden it).
        if screen.scrollback() == 0 && !screen.hide_cursor() {
            let (cy, cx) = screen.cursor_position();
            let rect = egui::Rect::from_min_size(
                origin + egui::vec2(cx as f32 * cell_w, cy as f32 * cell_h),
                egui::vec2(cell_w, cell_h),
            );
            painter.rect_filled(rect, 1.0, fg.linear_multiply(0.35));
        }

        if term.dead {
            painter.text(
                origin,
                egui::Align2::LEFT_TOP,
                "(no shell — see log)",
                font,
                fg.linear_multiply(0.6),
            );
        }
    });
}

/// The xterm-standard 16 base colours.
const ANSI_BASE: [[u8; 3]; 8] = [
    [0, 0, 0],
    [205, 0, 0],
    [0, 205, 0],
    [205, 205, 0],
    [0, 0, 238],
    [205, 0, 205],
    [0, 205, 205],
    [229, 229, 229],
];
const ANSI_BRIGHT: [[u8; 3]; 8] = [
    [127, 127, 127],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [92, 92, 255],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];

/// Resolve a vt100 colour (Default / 0-255 palette / truecolor) to RGB.
/// `bold` applies the xterm convention: bold text in a base colour uses the
/// bright slot instead.
fn ansi_rgb(idx: u8, bold: bool) -> [u8; 3] {
    match idx {
        0..=7 => {
            if bold {
                ANSI_BRIGHT[idx as usize]
            } else {
                ANSI_BASE[idx as usize]
            }
        }
        8..=15 => ANSI_BRIGHT[idx as usize - 8],
        16..=231 => {
            let n = idx - 16;
            let c = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            [c(n / 36), c((n % 36) / 6), c(n % 6)]
        }
        // 232..=255 grayscale ramp
        _ => {
            let v = 8 + 10 * (idx - 232);
            [v, v, v]
        }
    }
}

fn to_color32(rgb: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Resolved style for one cell: (fg, bg or None-for-default, bold, under,
/// strike). `None` bg means "the panel's own fill" — skip painting it.
struct CellStyle {
    fg: egui::Color32,
    bg: Option<egui::Color32>,
    bold: bool,
}

fn cell_style(
    cell: &vt100::Cell,
    default_fg: egui::Color32,
    default_bg: egui::Color32,
) -> CellStyle {
    let bold = cell.bold();
    let mut fg = match cell.fgcolor() {
        vt100::Color::Default => default_fg,
        vt100::Color::Idx(i) => to_color32(ansi_rgb(i, bold)),
        vt100::Color::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
    };
    let mut bg = match cell.bgcolor() {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(to_color32(ansi_rgb(i, false))),
        vt100::Color::Rgb(r, g, b) => Some(egui::Color32::from_rgb(r, g, b)),
    };
    if cell.inverse() {
        // Inverse video swaps the resolved colours; an unstyled fg/bg pair
        // still needs concrete values to swap into.
        let fg_resolved = fg;
        let bg_resolved = bg.unwrap_or(default_bg);
        fg = bg_resolved;
        bg = Some(fg_resolved);
    }
    if cell.dim() {
        fg = fg.linear_multiply(0.6);
    }
    CellStyle { fg, bg, bold }
}

/// Per-cell renderer: honours fg/bg colours (ANSI-16/256/truecolor), bold,
/// underline, strikethrough, and inverse video — the attributes TUIs like
/// opencode, vim, htop actually emit. Paint order per row: background runs,
/// then glyph runs grouped by style, then decoration lines.
fn draw_grid(
    painter: &egui::Painter,
    screen: &vt100::Screen,
    origin: egui::Pos2,
    cell_w: f32,
    cell_h: f32,
    font: egui::FontId,
    default_fg: egui::Color32,
    default_bg: egui::Color32,
) {
    let (nrows, ncols) = screen.size();
    // Grouping key: everything except the text itself.
    #[derive(PartialEq, Clone, Copy)]
    struct Key {
        fg: egui::Color32,
        bold: bool,
    }
    // Resolve "no bg" lazily per row via Option<Color32> compare.
    for r in 0..nrows {
        let y = origin.y + r as f32 * cell_h;

        // Pass 1 — background runs. Option<Color32> equality treats
        // "default" and "unpainted" as the same slot.
        let mut run_start = 0usize;
        let mut run_bg = None;
        let paint_run = |start: usize, end: usize, col: Option<egui::Color32>| {
            if let Some(c) = col {
                if end > start {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(origin.x + start as f32 * cell_w, y),
                            egui::vec2((end - start) as f32 * cell_w, cell_h),
                        ),
                        0.0,
                        c,
                    );
                }
            }
        };
        for c in 0..ncols {
            let Some(cell) = screen.cell(r, c) else {
                continue;
            };
            let st = cell_style(cell, default_fg, default_bg);
            if st.bg != run_bg {
                paint_run(run_start, c as usize, run_bg);
                run_start = c as usize;
                run_bg = st.bg;
            }
        }
        paint_run(run_start, ncols as usize, run_bg);

        // Pass 2 — glyph runs grouped by (fg,bold); bold = faux double-draw
        // since no bold face is registered in egui's default font set.
        let mut run_text = String::new();
        let mut run_col = 0usize;
        let mut cur: Option<Key> = None;
        let flush = |text: &mut String, start: usize, key: Key, y: f32| {
            if text.is_empty() {
                return;
            }
            let x = origin.x + start as f32 * cell_w;
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                &*text,
                font.clone(),
                key.fg,
            );
            if key.bold {
                painter.text(
                    egui::pos2(x + 0.7, y),
                    egui::Align2::LEFT_TOP,
                    &*text,
                    font.clone(),
                    key.fg,
                );
            }
            text.clear();
        };
        for c in 0..ncols {
            let Some(cell) = screen.cell(r, c) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let st = cell_style(cell, default_fg, default_bg);
            let key = Key {
                fg: st.fg,
                bold: st.bold,
            };
            if cur != Some(key) {
                if let Some(k) = cur {
                    flush(&mut run_text, run_col, k, y);
                }
                cur = Some(key);
                run_col = c as usize;
            }
            // Wide glyphs occupy two cells; egui spaces them naturally.
            let text = cell.contents();
            run_text.push_str(if text.is_empty() { " " } else { text });
        }
        if let Some(k) = cur {
            flush(&mut run_text, run_col, k, y);
        }

        // Pass 3 — underline per contiguous flag run (vt100 has no strike).
        for c in 0..ncols {
            let Some(cell) = screen.cell(r, c) else {
                continue;
            };
            if !cell.underline() {
                continue;
            }
            let st = cell_style(cell, default_fg, default_bg);
            let x0 = origin.x + c as f32 * cell_w;
            painter.hline(
                x0..=x0 + cell_w,
                y + cell_h - 2.0,
                egui::Stroke::new(1.0, st.fg),
            );
        }
    }
}

/// Translate egui pointer events into xterm mouse-report bytes when the
/// application enabled a mouse protocol (1000/1002/1003 + 1006 SGR). TUIs
/// like opencode rely on this for clicks, drags and wheel scrolling.
fn encode_pointer(
    ev: &egui::Event,
    mode: vt100::MouseProtocolMode,
    sgr: bool,
    origin: egui::Pos2,
    cw: f32,
    ch: f32,
    pointer_pos: egui::Pos2,
    down: &mut Option<u8>,
    out: &mut Vec<u8>,
) {
    // Pixel → 1-based cell coordinate.
    let cell = |pos: egui::Pos2| -> Option<(u16, u16)> {
        let x = (pos.x - origin.x) / cw;
        let y = (pos.y - origin.y) / ch;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        Some((x as u16 + 1, y as u16 + 1))
    };
    let emit = |b: u8, x: u16, y: u16, release: bool, out: &mut Vec<u8>| {
        if sgr {
            let tail = if release { b'm' } else { b'M' };
            out.extend_from_slice(format!("\x1b[<{b};{x};{y}").as_bytes());
            out.push(tail);
        } else {
            // Legacy X10/UTF8 byte encoding (coords clamp to 223).
            let x = x.min(223) as u8;
            let y = y.min(223) as u8;
            out.extend_from_slice(&[0x1b, b'[', b'M', 32 + b, 32 + x, 32 + y]);
        }
    };

    match ev {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            ..
        } => {
            let Some((x, y)) = cell(*pos) else { return };
            let code = match button {
                egui::PointerButton::Primary => 0,
                egui::PointerButton::Middle => 1,
                egui::PointerButton::Secondary => 2,
                _ => return,
            };
            if *pressed {
                emit(code, x, y, false, out);
                *down = Some(code);
            } else if mode != vt100::MouseProtocolMode::Press {
                // X10 reports only presses; VT200+ reports release as b+3.
                emit(3, x, y, true, out);
                *down = None;
            }
        }
        egui::Event::PointerMoved(pos) => {
            let report = match (mode, *down) {
                // Button held + motion-capable mode → 32+button.
                (vt100::MouseProtocolMode::ButtonMotion, Some(b))
                | (vt100::MouseProtocolMode::AnyMotion, Some(b)) => Some(32 + b),
                // AnyMotion, no button → 35 (motion-with-no-button code).
                (vt100::MouseProtocolMode::AnyMotion, None) => Some(35),
                _ => None,
            };
            if let Some(b) = report {
                if let Some((x, y)) = cell(*pos) {
                    emit(b, x, y, false, out);
                }
            }
        }
        egui::Event::MouseWheel {
            unit: _,
            delta,
            modifiers: _,
            ..
        } => {
            // Wheel = buttons 64/65 at the pointer's cell.
            if let Some((x, y)) = cell(pointer_pos) {
                let steps = delta.y.abs().max(1.0) as usize;
                let b = if delta.y > 0.0 { 64 } else { 65 };
                for _ in 0..steps.min(8) {
                    emit(b, x, y, false, out);
                }
            }
        }
        _ => {}
    }
}

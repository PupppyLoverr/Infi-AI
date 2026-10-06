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
        let mut cmd = CommandBuilder::new(&shell);
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
        ui.ctx().input(|i| {
            for ev in &i.events {
                encode_event(ev, &mut out);
            }
        });
        if !out.is_empty() {
            let _ = self.writer.write_all(&out);
            let _ = self.writer.flush();
        }
    }
}

fn encode_event(ev: &egui::Event, out: &mut Vec<u8>) {
    use egui::Key::*;
    match ev {
        egui::Event::Text(t) => {
            out.extend_from_slice(t.as_bytes());
        }
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            let ctrl = modifiers.ctrl;
            match key {
                Enter => out.push(b'\r'),
                Backspace => out.push(0x7f),
                Tab => out.push(b'\t'),
                Escape => out.push(0x1b),
                ArrowUp => out.extend_from_slice(b"\x1b[A"),
                ArrowDown => out.extend_from_slice(b"\x1b[B"),
                ArrowRight => out.extend_from_slice(b"\x1b[C"),
                ArrowLeft => out.extend_from_slice(b"\x1b[D"),
                Home => out.extend_from_slice(b"\x1b[H"),
                End => out.extend_from_slice(b"\x1b[F"),
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

        let painter = ui.painter();
        let origin = ui.cursor().min + egui::vec2(PAD, PAD);
        let fg = ui
            .visuals()
            .override_text_color
            .unwrap_or(egui::Color32::WHITE);
        let font = egui::FontId::monospace(FONT);

        let screen = term.parser.screen();
        // rows() yields every displayed row as text: scrollback lines first
        // (when scrolled), then the live grid.
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

        // Block cursor at the parser's cursor position (only when not
        // scrolled back).
        if screen.scrollback() == 0 {
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

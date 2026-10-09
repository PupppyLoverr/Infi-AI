//! Ask mode answers: runs `opencode run <question>` (the image ships
//! opencode; its provider config is the user's own) and streams stdout
//! into the Search or Ask card.

use std::io::Read;
use std::process::{Command, Stdio};

pub enum Event {
    Started(u32),
    Chunk(String),
    Done(Result<(), String>),
}

/// Answer `question` on a worker thread; events are tagged with `id` so
/// a superseded question's late output is ignored.
pub fn start(question: String, id: u64, tx: calloop::channel::Sender<(u64, Event)>) {
    std::thread::spawn(move || {
        let child = Command::new("opencode")
            .args(["run", &question])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let msg = if e.kind() == std::io::ErrorKind::NotFound {
                    "opencode is not installed".to_string()
                } else {
                    format!("could not start opencode: {e}")
                };
                let _ = tx.send((id, Event::Done(Err(msg))));
                return;
            }
        };
        let _ = tx.send((id, Event::Started(child.id())));
        let mut err = String::new();
        let stderr = child.stderr.take();
        let err_thread = std::thread::spawn(move || {
            if let Some(mut s) = stderr {
                let _ = s.read_to_string(&mut err);
            }
            err
        });
        let mut out = child.stdout.take().expect("piped stdout");
        let mut buf = [0u8; 1024];
        let mut pending = Vec::new();
        loop {
            match out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let valid = match std::str::from_utf8(&pending) {
                        Ok(_) => pending.len(),
                        Err(e) => e.valid_up_to(),
                    };
                    let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
                    pending.drain(..valid);
                    if !text.is_empty() && tx.send((id, Event::Chunk(strip_ansi(&text)))).is_err() {
                        let _ = child.kill();
                        return;
                    }
                }
            }
        }
        let status = child.wait();
        let err = err_thread.join().unwrap_or_default();
        let res = match status {
            Ok(s) if s.success() => Ok(()),
            _ => Err(strip_ansi(&err)
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("opencode exited with an error")
                .trim()
                .to_string()),
        };
        let _ = tx.send((id, Event::Done(res)));
    });
}

/// Drop CSI/OSC escape sequences (opencode colours its output).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match it.next() {
            Some('[') => {
                for c in it.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = it.next() {
                    if c == '\x07' || (c == '\x1b' && it.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Greedy word wrap by measured width.
pub fn wrap(text: &str, max_w: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split(' ') {
            let cand = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if measure(&cand) <= max_w || cur.is_empty() {
                cur = cand;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        lines.push(cur);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_colour_and_wraps() {
        assert_eq!(strip_ansi("\x1b[1;32mok\x1b[0m\r\n\x1b]0;t\x07x"), "ok\nx");
        let w = wrap("one two three four\n\nfive", 9.0, |s| s.len() as f32);
        assert_eq!(w, ["one two", "three", "four", "", "five"]);
    }
}

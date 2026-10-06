//! Client of the compositor's IPC socket (`cosmos-ipc`).

use std::{io::Read, os::unix::net::UnixStream};

use cosmos_ipc::{Event, Request};

#[derive(Debug)]
pub struct IpcClient {
    stream: Option<UnixStream>,
    buf: Vec<u8>,
}

impl Default for IpcClient {
    fn default() -> Self {
        Self {
            stream: None,
            buf: Vec::new(),
        }
    }
}

impl IpcClient {
    /// (Re)connect; called on startup and after a disconnect.
    pub fn connect(&mut self) -> Option<UnixStream> {
        let stream = UnixStream::connect(cosmos_ipc::socket_path()).ok()?;
        stream.set_nonblocking(true).ok()?;
        let reader = stream.try_clone().ok()?;
        self.buf.clear();
        let _ = cosmos_ipc::write_message(&mut stream.try_clone().ok()?, &Request::Subscribe);
        self.stream = Some(stream);
        Some(reader)
    }

    /// True when connected.
    pub fn connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Drain all complete events queued in `buf`.
    pub fn poll_events(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            let Some(pos) = self.buf.iter().position(|b| *b == b'\n') else {
                break;
            };
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let Ok(text) = String::from_utf8(line) else {
                continue;
            };
            if let Ok(ev) = serde_json::from_str::<Event>(text.trim()) {
                out.push(ev);
            }
        }
        out
    }

    /// Pull new bytes. Returns false on EOF/error → caller should reconnect.
    pub fn read(&mut self) -> bool {
        let Some(stream) = self.stream.as_mut() else {
            return false;
        };
        let mut buf = [0u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    self.stream = None;
                    return false;
                }
                Ok(n) => self.buf.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return true,
                Err(_) => {
                    self.stream = None;
                    return false;
                }
            }
        }
    }

    pub fn send(&mut self, req: &Request) {
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        if cosmos_ipc::write_message(stream, req).is_err() {
            self.stream = None;
        }
    }
}

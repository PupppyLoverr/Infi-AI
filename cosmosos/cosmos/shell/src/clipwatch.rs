//! Clipboard watch + write for the shell — a wlr-data-control client.
//! Watches the seat's clipboard selections into a history ring the island
//! shows, and re-sets the selection when a history row is picked.

use std::os::unix::io::{AsFd, AsRawFd, FromRawFd, OwnedFd};

use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

use crate::ShellState;

/// MIME types we read off the clipboard, in preference order.
pub const TEXT_MIMES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
];

pub const HISTORY_MAX: usize = 10;

/// Push a copied string into the history ring (dedup, most recent first).
pub fn note_clip(state: &mut ShellState, text: String) {
    let text = text.trim_end_matches('\0').to_string();
    if text.is_empty() {
        return;
    }
    state.clip_history.retain(|t| t != &text);
    state.clip_history.push_front(text);
    while state.clip_history.len() > HISTORY_MAX {
        state.clip_history.pop_back();
    }
    state.panel_dirty = true;
    state.island_dirty = true;
}

/// Read one offer's text payload on a helper thread (the owner may take
/// a moment to write), then push it into the history.
fn read_offer(state: &mut ShellState, offer: &ZwlrDataControlOfferV1) {
    let Some(mime) = state
        .clip_offers
        .get(&offer.id())
        .and_then(|mimes| TEXT_MIMES.iter().find(|m| mimes.iter().any(|t| t == *m)))
    else {
        return;
    };
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    let (read_end, write_end) = unsafe {
        (
            std::fs::File::from_raw_fd(fds[0]),
            OwnedFd::from_raw_fd(fds[1]),
        )
    };
    offer.receive(mime.to_string(), write_end.as_fd());
    let tx = state.clip_tx.clone();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut f = read_end;
        // Give the owner up to 2s to write; nonblocking poll so a dead
        // source can't hang the reader thread forever.
        unsafe {
            let fd = f.as_raw_fd();
            libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK);
        }
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            match f.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
        if let Ok(text) = String::from_utf8(buf) {
            if !text.trim().is_empty() {
                let _ = tx.send(text);
            }
        }
    });
}

/// Re-set the seat's clipboard to `text` — used when a history row is
/// picked in the island.
pub fn set_clipboard(state: &mut ShellState, text: String) {
    let (Some(mgr), Some(dev)) = (state.clip_manager.as_ref(), state.clip_device.as_ref()) else {
        return;
    };
    let source = mgr.create_data_source(&state.qh, ());
    for mime in TEXT_MIMES {
        source.offer(mime.to_string());
    }
    state.clip_pending_text = text;
    state.clip_source = Some(source.clone());
    dev.set_selection(Some(&source));
}

/// Bind the data-control manager + device for the shell's seat.
/// `globals` is passed in because `ShellState` keeps no handle to it.
pub fn init(state: &mut ShellState, globals: &wayland_client::globals::GlobalList) {
    let Ok(mgr) = globals.bind::<ZwlrDataControlManagerV1, _, _>(&state.qh, 1..=1, ()) else {
        tracing::warn!("clipwatch: zwlr_data_control_manager_v1 unavailable");
        return;
    };
    if let Some(seat) = state.seat_state.seats().next() {
        let dev = mgr.get_data_device(&seat, &state.qh, ());
        state.clip_device = Some(dev);
    }
    state.clip_manager = Some(mgr);
}

// ---------------- wayland dispatch ----------------

impl Dispatch<ZwlrDataControlManagerV1, ()> for ShellState {
    fn event(
        _state: &mut ShellState,
        _mgr: &ZwlrDataControlManagerV1,
        _event: <ZwlrDataControlManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<ShellState>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for ShellState {
    fn event(
        state: &mut ShellState,
        _dev: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<ShellState>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id: data_offer } => {
                state.clip_offers.insert(data_offer.id(), Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id } => {
                if let Some(offer) = id {
                    read_offer(state, &offer);
                    state.clip_offers.remove(&offer.id());
                }
            }
            zwlr_data_control_device_v1::Event::Finished
            | zwlr_data_control_device_v1::Event::PrimarySelection { .. } => {}
            _ => {}
        }
    }

    /// Opcode 0 (`data_offer`) creates a `zwlr_data_control_offer_v1`
    /// child — without this the first external `wl-copy` panics the
    /// whole shell on `event_created_child`.
    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<ShellState>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        match opcode {
            0 => qhandle.make_data::<ZwlrDataControlOfferV1, ()>(()),
            _ => unreachable!("zwlr_data_control_device_v1 child opcode {opcode}"),
        }
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for ShellState {
    fn event(
        state: &mut ShellState,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<ShellState>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            state
                .clip_offers
                .entry(offer.id())
                .or_default()
                .push(mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for ShellState {
    fn event(
        state: &mut ShellState,
        _source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<ShellState>,
    ) {
        use std::io::Write;
        match event {
            zwlr_data_control_source_v1::Event::Send { fd, .. } => {
                let mut file = std::fs::File::from(fd);
                let _ = file.write_all(state.clip_pending_text.as_bytes());
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                state.clip_source = None;
            }
            _ => {}
        }
    }
}

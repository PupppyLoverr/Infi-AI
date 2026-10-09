//! Clipboard watch + write for the shell — a wlr-data-control client.
//! Watches the seat's clipboard selections (text and PNG images) into a
//! history ring the island shows, and re-sets the selection when a history
//! card is picked.

use std::os::unix::io::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;

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

/// Image MIME we read and re-offer; browsers, screenshot tools and image
/// editors all put PNG on the clipboard.
pub const IMAGE_MIME: &str = "image/png";

pub const HISTORY_MAX: usize = 10;

/// Largest clipboard payload we keep (a 4K screenshot PNG is ~10 MiB).
const MAX_BYTES: usize = 32 << 20;

/// Island clipboard-card thumbnail bound in physical pixels (2× the card).
pub const THUMB_W: u32 = 152;
pub const THUMB_H: u32 = 136;

/// One clipboard history entry.
#[derive(Clone)]
pub enum Clip {
    Text(String),
    Image {
        png: Arc<Vec<u8>>,
        thumb: Arc<tiny_skia::Pixmap>,
    },
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Clip::Text(t) => f.debug_tuple("Text").field(t).finish(),
            Clip::Image { png, .. } => write!(f, "Image({} bytes)", png.len()),
        }
    }
}

impl PartialEq for Clip {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Clip::Text(a), Clip::Text(b)) => a == b,
            (Clip::Image { png: a, .. }, Clip::Image { png: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// Decode a PNG payload into a history entry with its card thumbnail.
pub fn image_clip(png: Vec<u8>) -> Option<Clip> {
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let thumb = crate::preview::fit_premultiplied(&img, THUMB_W, THUMB_H)?;
    Some(Clip::Image {
        png: Arc::new(png),
        thumb: Arc::new(thumb),
    })
}

/// Push a copied entry into the history ring (dedup, most recent first).
pub fn note_clip(state: &mut ShellState, clip: Clip) {
    let clip = match clip {
        Clip::Text(t) => {
            let t = t.trim_end_matches('\0').to_string();
            if t.trim().is_empty() {
                return;
            }
            Clip::Text(t)
        }
        img => img,
    };
    state.clip_history.retain(|c| c != &clip);
    state.clip_history.push_front(clip);
    while state.clip_history.len() > HISTORY_MAX {
        state.clip_history.pop_back();
    }
    state.panel_dirty = true;
    state.island_dirty = true;
}

/// Read one offer's payload — text if offered, else a PNG image — on a
/// helper thread (the owner may take a moment to write), then push it into
/// the history.
fn read_offer(state: &mut ShellState, offer: &ZwlrDataControlOfferV1) {
    let Some(mimes) = state.clip_offers.get(&offer.id()) else {
        return;
    };
    let Some(mime) = TEXT_MIMES
        .iter()
        .chain(std::iter::once(&IMAGE_MIME))
        .find(|m| mimes.iter().any(|t| t == *m))
        .copied()
    else {
        return;
    };
    let is_image = mime == IMAGE_MIME;
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
                Ok(_) if buf.len() > MAX_BYTES => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
        let clip = if is_image {
            image_clip(buf)
        } else {
            String::from_utf8(buf).ok().map(Clip::Text)
        };
        if let Some(clip) = clip {
            let _ = tx.send(clip);
        }
    });
}

/// Re-set the seat's clipboard to `clip` — used when a history card is
/// picked in the island.
pub fn set_clipboard(state: &mut ShellState, clip: Clip) {
    let (Some(mgr), Some(dev)) = (state.clip_manager.as_ref(), state.clip_device.as_ref()) else {
        return;
    };
    let source = mgr.create_data_source(&state.qh, ());
    match &clip {
        Clip::Text(_) => {
            for mime in TEXT_MIMES {
                source.offer(mime.to_string());
            }
        }
        Clip::Image { .. } => source.offer(IMAGE_MIME.to_string()),
    }
    state.clip_pending = Some(clip);
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
                let bytes: Arc<Vec<u8>> = match &state.clip_pending {
                    Some(Clip::Text(t)) => Arc::new(t.clone().into_bytes()),
                    Some(Clip::Image { png, .. }) => png.clone(),
                    None => return,
                };
                // Off the main thread: a multi-MiB PNG fills the pipe and
                // would stall the shell until the paster drains it.
                std::thread::spawn(move || {
                    let mut file = std::fs::File::from(fd);
                    let _ = file.write_all(&bytes);
                });
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                state.clip_source = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([40, 120, 220, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn png_clips_get_a_card_thumbnail() {
        let Some(Clip::Image { thumb, png: bytes }) = image_clip(png(1600, 900)) else {
            panic!("png did not decode");
        };
        assert!(thumb.width() <= THUMB_W && thumb.height() <= THUMB_H);
        assert_eq!(thumb.width(), THUMB_W);
        assert_eq!(
            Clip::Image {
                png: bytes.clone(),
                thumb: thumb.clone()
            },
            image_clip((*bytes).clone()).unwrap()
        );
        assert!(image_clip(b"not a png".to_vec()).is_none());
    }
}

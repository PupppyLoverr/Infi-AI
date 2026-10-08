//! PipeWire producer for the ScreenCast portal.
//!
//! `start_cast` spawns a dedicated thread running a `pipewire` main
//! loop with one producer node ("cosmos-desktop"). Frames come from the
//! compositor's IPC screenshot path — real pixels, decoded to BGRx —
//! shared with the stream's `process` callback through a mutex. The
//! caller blocks until the stream is connected and reports back the
//! node id to put in the portal `streams` result.

use std::sync::{Arc, Mutex};

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;
use tracing::{error, info, warn};

/// Shared state between the capture pump, the PipeWire `process`
/// callback, and the portal `Session` object's `Close` method.
pub(crate) struct CastFrame {
    /// BGRx pixel bytes at `stride` bytes/row.
    pub pixels: Vec<u8>,
    pub stride: usize,
    /// Set on session Close; the pump stops and the mainloop quits
    /// (the pw thread's process callback notices the flag and quits
    /// its own mainloop — the loop's Weak handle isn't Send, so it
    /// lives inside the listener closure, not here).
    pub done: bool,
}

impl CastFrame {
    pub(crate) fn empty() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            pixels: Vec::new(),
            stride: 0,
            done: false,
        }))
    }
}

/// Ask the producer to stop: marks `done`, and — once the mainloop is
/// registered — quits it so the thread unwinds.
pub(crate) fn stop_cast(frame: &Arc<Mutex<CastFrame>>) {
    if let Ok(mut f) = frame.lock() {
        f.done = true;
    }
}

/// Capture pump: repeated IPC screenshots → decoded BGRx frames.
fn pump_frames(shared: Arc<Mutex<CastFrame>>, w: usize, h: usize, stride: usize) {
    loop {
        {
            if shared.lock().map(|f| f.done).unwrap_or(true) {
                return;
            }
        }
        if let Ok(uri) = super::ipc_screenshot() {
            let path = uri.trim_start_matches("file://").to_string();
            match image::open(&path) {
                Ok(img) => {
                    let rgba_img = img.to_rgba8();
                    let rgba = rgba_img.as_raw();
                    let mut bgrx = vec![0u8; stride * h];
                    for y in 0..h {
                        let row = y * stride;
                        for x in 0..w {
                            let src = (y * w + x) * 4;
                            let dst = row + x * 4;
                            if src + 3 < rgba.len() && dst + 3 < bgrx.len() {
                                bgrx[dst] = rgba[src + 2];
                                bgrx[dst + 1] = rgba[src + 1];
                                bgrx[dst + 2] = rgba[src];
                                bgrx[dst + 3] = 0xff;
                            }
                        }
                    }
                    if let Ok(mut f) = shared.lock() {
                        f.pixels = bgrx;
                    }
                }
                Err(e) => warn!("screencast: decode {} failed: {e}", path),
            }
        }
        // ~4 fps pump — the process callback repeats the last frame
        // whenever PipeWire wants data faster than we can capture.
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Start a PipeWire producer streaming `w`x`h` BGRx frames of the
/// desktop. Returns the node id plus the shared frame state (kept by
/// the portal session so `Close` can stop the cast).
pub(crate) fn start_cast(w: i32, h: i32) -> Result<(u32, Arc<Mutex<CastFrame>>), String> {
    let stride = (w as usize) * 4;
    let shared = CastFrame::empty();
    shared.lock().map_err(|e| e.to_string())?.stride = stride;
    let (tx, rx) = std::sync::mpsc::channel::<Result<u32, String>>();
    let shared2 = shared.clone();

    std::thread::spawn(move || {
        let run = || -> Result<(), String> {
            pipewire::init();
            let mainloop =
                pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("pw mainloop: {e}"))?;
            let context =
                pw::context::ContextRc::new(&mainloop, None).map_err(|e| format!("pw ctx: {e}"))?;
            let core = context
                .connect_rc(None)
                .map_err(|e| format!("pw connect: {e}"))?;

            let stream = pw::stream::StreamBox::new(
                &core,
                "cosmos-desktop",
                pw::properties::properties! {
                    *pw::keys::MEDIA_TYPE => "Video",
                    *pw::keys::MEDIA_CATEGORY => "Capture",
                    *pw::keys::MEDIA_ROLE => "Screen",
                },
            )
            .map_err(|e| format!("pw stream: {e}"))?;

            let shared_cb = shared2.clone();
            let weak = mainloop.downgrade();
            let listener = stream
                .add_local_listener_with_user_data(())
                .process(move |stream, _ud| {
                    let done = shared_cb.lock().map(|f| f.done).unwrap_or(true);
                    if done {
                        if let Some(m) = weak.upgrade() {
                            m.quit();
                        }
                        return;
                    }
                    let mut buffer = match stream.dequeue_buffer() {
                        Some(b) => b,
                        None => return,
                    };
                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return; // buffer requeues itself on drop
                    }
                    let data = &mut datas[0];
                    if let Some(dest) = data.data() {
                        // `data()` hands back the full capacity (maxsize)
                        if let Ok(f) = shared_cb.lock() {
                            let copy_len = f.pixels.len().min(dest.len());
                            dest[..copy_len].copy_from_slice(&f.pixels[..copy_len]);
                            let chunk = data.chunk_mut();
                            *chunk.size_mut() = copy_len as u32;
                            *chunk.stride_mut() = f.stride as i32;
                            *chunk.offset_mut() = 0;
                        }
                    }
                    drop(buffer);
                })
                .register()
                .map_err(|e| format!("pw listener: {e}"))?;

            let obj = spa::pod::object!(
                spa::utils::SpaTypes::ObjectParamFormat,
                spa::param::ParamType::EnumFormat,
                spa::pod::property!(
                    spa::param::format::FormatProperties::MediaType,
                    Id,
                    spa::param::format::MediaType::Video
                ),
                spa::pod::property!(
                    spa::param::format::FormatProperties::MediaSubtype,
                    Id,
                    spa::param::format::MediaSubtype::Raw
                ),
                spa::pod::property!(
                    spa::param::format::FormatProperties::VideoFormat,
                    Id,
                    spa::param::video::VideoFormat::BGRx
                ),
                spa::pod::property!(
                    spa::param::format::FormatProperties::VideoSize,
                    Rectangle,
                    spa::utils::Rectangle {
                        width: w as u32,
                        height: h as u32
                    }
                ),
                spa::pod::property!(
                    spa::param::format::FormatProperties::VideoFramerate,
                    Choice,
                    Range,
                    Fraction,
                    spa::utils::Fraction { num: 30, denom: 1 },
                    spa::utils::Fraction { num: 0, denom: 1 },
                    spa::utils::Fraction {
                        num: 1000,
                        denom: 1
                    }
                ),
            );
            let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &spa::pod::Value::Object(obj),
            )
            .map_err(|e| format!("pod serialize: {e:?}"))?
            .0
            .into_inner();
            let pod = Pod::from_bytes(&values).ok_or_else(|| "pod bytes".to_string())?;
            let mut params = [pod];

            stream
                .connect(
                    spa::utils::Direction::Output,
                    None,
                    pw::stream::StreamFlags::ALLOC_BUFFERS | pw::stream::StreamFlags::MAP_BUFFERS,
                    &mut params,
                )
                .map_err(|e| format!("pw stream connect: {e}"))?;

            let node_id = stream.node_id();
            info!("screencast: pw node {node_id} streaming {w}x{h}");
            let _ = tx.send(Ok(node_id));
            // `listener` and `stream` stay alive in this scope while
            // the mainloop drives them; `core`/`context` live here too
            // since `stream` borrows them.
            mainloop.run();
            drop((listener, stream));
            Ok(())
        };

        if let Err(e) = run() {
            error!("screencast: {e}");
            let _ = tx.send(Err(e));
        }
    });

    match rx.recv_timeout(std::time::Duration::from_secs(15)) {
        Ok(Ok(node_id)) => {
            let pump_shared = shared.clone();
            let pw_w = w as usize;
            let ph = h as usize;
            std::thread::spawn(move || pump_frames(pump_shared, pw_w, ph, stride));
            Ok((node_id, shared))
        }
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("pw producer timed out: {e}")),
    }
}

//! Preview pane and Quick Look content: images decoded and downscaled
//! off the UI thread, text files read as a bounded UTF-8 head.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];
/// Longest edge of the decoded preview texture.
const MAX_PX: u32 = 1024;
pub const TEXT_BYTES: usize = 16 * 1024;

pub enum Content {
    Loading,
    /// Texture plus the source image's real pixel size.
    Image(egui::TextureHandle, [u32; 2]),
    Text(String),
    Folder(usize),
    None,
}

enum Loaded {
    Image(egui::ColorImage, [u32; 2]),
    Text(String),
    Folder(usize),
    None,
}

pub struct Preview {
    pub path: PathBuf,
    pub content: Content,
    rx: Option<mpsc::Receiver<Loaded>>,
}

impl Preview {
    pub fn start(ctx: &egui::Context, path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let (p, c) = (path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(load(&p));
            c.request_repaint();
        });
        Preview {
            path,
            content: Content::Loading,
            rx: Some(rx),
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(loaded) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.rx = None;
        self.content = match loaded {
            Loaded::Image(ci, dims) => Content::Image(
                ctx.load_texture(
                    format!("files-preview:{}", self.path.display()),
                    ci,
                    egui::TextureOptions::LINEAR,
                ),
                dims,
            ),
            Loaded::Text(t) => Content::Text(t),
            Loaded::Folder(n) => Content::Folder(n),
            Loaded::None => Content::None,
        };
    }
}

pub fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

fn load(path: &Path) -> Loaded {
    if path.is_dir() {
        return std::fs::read_dir(path)
            .map(|d| Loaded::Folder(d.filter(|e| e.is_ok()).count()))
            .unwrap_or(Loaded::None);
    }
    if is_image(path) {
        return match image::open(path) {
            Ok(img) => {
                let dims = [img.width(), img.height()];
                let rgba = img.thumbnail(MAX_PX, MAX_PX).to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                Loaded::Image(
                    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                    dims,
                )
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), "files: preview decode failed: {e}");
                Loaded::None
            }
        };
    }
    text_head(path).map_or(Loaded::None, Loaded::Text)
}

/// The first TEXT_BYTES of `path` if it reads as text: no NUL bytes and
/// valid UTF-8, except a multibyte char cut off by the byte limit.
pub fn text_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::with_capacity(TEXT_BYTES);
    std::fs::File::open(path)
        .ok()?
        .take(TEXT_BYTES as u64)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.contains(&0) {
        return None;
    }
    match std::str::from_utf8(&buf) {
        Ok(s) => Some(s.to_string()),
        Err(e) if e.error_len().is_none() => {
            Some(String::from_utf8_lossy(&buf[..e.valid_up_to()]).into_owned())
        }
        Err(_) => None,
    }
}

/// `rect` shrunk to `size`'s aspect ratio, centred, never upscaled.
pub fn fit(rect: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let s = (rect.width() / size.x).min(rect.height() / size.y).min(1.0);
    egui::Rect::from_center_size(rect.center(), size * s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cosmos-files-{}-{name}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn text_head_rules() {
        assert_eq!(
            text_head(&tmp("a.txt", b"hello\n")).as_deref(),
            Some("hello\n")
        );
        assert_eq!(text_head(&tmp("b.bin", b"PK\x03\x04\0\0")), None);
        // "é" split by the byte cap keeps the valid prefix.
        let mut long = vec![b'a'; TEXT_BYTES - 1];
        long.extend_from_slice("é".as_bytes());
        let head = text_head(&tmp("c.txt", &long)).unwrap();
        assert_eq!(head.len(), TEXT_BYTES - 1);
    }

    #[test]
    fn image_ext_and_fit() {
        assert!(is_image(Path::new("/x/Photo.JPG")));
        assert!(!is_image(Path::new("/x/notes.md")));
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
        let f = fit(r, egui::vec2(400.0, 400.0));
        assert_eq!(f.size(), egui::vec2(100.0, 100.0));
        assert_eq!(f.center(), r.center());
        assert_eq!(
            fit(r, egui::vec2(20.0, 10.0)).size(),
            egui::vec2(20.0, 10.0)
        );
    }
}

//! Right-hand preview for a Files result in Search or Ask: a PNG
//! thumbnail, the first lines of a text file, or just its metadata.

use std::path::Path;
use tiny_skia::Pixmap;

pub const THUMB_W: u32 = 248;
pub const THUMB_H: u32 = 150;
const TEXT_LINES: usize = 9;
const TEXT_COLS: usize = 38;

pub enum Body {
    Image(Pixmap),
    Text(Vec<String>),
    None,
}

pub struct FilePreview {
    pub path: String,
    pub name: String,
    /// "PNG image · 1.2 MB" / "Folder · 12 items".
    pub info: String,
    pub modified: String,
    pub body: Body,
}

pub fn load(path: &str) -> FilePreview {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).ok();
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let info = if is_dir {
        let n = std::fs::read_dir(p).map(|d| d.count()).unwrap_or(0);
        format!("Folder · {n} item{}", if n == 1 { "" } else { "s" })
    } else {
        format!("{} · {}", kind(&ext), human_size(size))
    };
    let modified = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format!("Modified {}", local_time(d.as_secs() as i64)))
        .unwrap_or_default();
    let body = if is_dir {
        Body::None
    } else if ext == "png" && size < 32 << 20 {
        thumbnail(p).map(Body::Image).unwrap_or(Body::None)
    } else if size < 1 << 20 {
        text_head(p).map(Body::Text).unwrap_or(Body::None)
    } else {
        Body::None
    };
    FilePreview {
        path: path.to_string(),
        name,
        info,
        modified,
        body,
    }
}

fn kind(ext: &str) -> String {
    match ext {
        "png" => "PNG image".into(),
        "jpg" | "jpeg" => "JPEG image".into(),
        "txt" => "Plain text".into(),
        "md" => "Markdown".into(),
        "rs" => "Rust source".into(),
        "toml" => "TOML".into(),
        "json" => "JSON".into(),
        "sh" => "Shell script".into(),
        "pdf" => "PDF document".into(),
        "" => "File".into(),
        e => format!("{} file", e.to_ascii_uppercase()),
    }
}

pub fn human_size(b: u64) -> String {
    const U: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if b < 1000 {
        return format!("{b} bytes");
    }
    let mut v = b as f64 / 1000.0;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}

fn local_time(secs: i64) -> String {
    // SAFETY: localtime_r only writes the provided tm.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = secs as libc::time_t;
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return String::new();
    }
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {} {:02}:{:02}",
        tm.tm_mday,
        MON[tm.tm_mon.clamp(0, 11) as usize],
        tm.tm_year + 1900,
        tm.tm_hour,
        tm.tm_min
    )
}

/// The image fitted into THUMB_W×THUMB_H, premultiplied for tiny-skia.
fn thumbnail(p: &Path) -> Option<Pixmap> {
    let img = image::open(p).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let s = (THUMB_W as f32 / w as f32)
        .min(THUMB_H as f32 / h as f32)
        .min(1.0);
    let (tw, th) = (
        ((w as f32 * s) as u32).max(1),
        ((h as f32 * s) as u32).max(1),
    );
    let small = image::imageops::thumbnail(&img, tw, th);
    let mut data = small.into_raw();
    for i in (0..data.len()).step_by(4) {
        let a = data[i + 3] as u16;
        for c in &mut data[i..i + 3] {
            *c = ((*c as u16 * a + 127) / 255) as u8;
        }
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(tw, th)?)
}

/// First lines of a UTF-8 text file; None for binary content.
fn text_head(p: &Path) -> Option<Vec<String>> {
    use std::io::Read;
    let mut buf = vec![0u8; 4096];
    let n = std::fs::File::open(p).ok()?.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.contains(&0) {
        return None;
    }
    let s = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        // A multibyte char cut at the 4 KiB boundary is still text.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&buf[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    let lines: Vec<String> = s
        .lines()
        .take(TEXT_LINES)
        .map(|l| l.replace('\t', "    ").chars().take(TEXT_COLS).collect())
        .collect();
    (!lines.is_empty()).then_some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_png_and_folder_previews() {
        let d = std::env::temp_dir().join(format!("cosmos-preview-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let t = d.join("notes.md");
        std::fs::write(&t, "# Title\n\tindented line\nthird").unwrap();
        let p = load(t.to_str().unwrap());
        assert_eq!(p.name, "notes.md");
        assert!(p.info.starts_with("Markdown · "), "{}", p.info);
        match p.body {
            Body::Text(l) => assert_eq!(l, ["# Title", "    indented line", "third"]),
            _ => panic!("expected text"),
        }
        let img = d.join("pic.png");
        image::RgbaImage::from_pixel(800, 400, image::Rgba([200, 10, 10, 255]))
            .save(&img)
            .unwrap();
        match load(img.to_str().unwrap()).body {
            Body::Image(px) => assert_eq!((px.width(), px.height()), (248, 124)),
            _ => panic!("expected image"),
        }
        std::fs::write(d.join("bin"), [0u8, 1, 2]).unwrap();
        assert!(matches!(
            load(d.join("bin").to_str().unwrap()).body,
            Body::None
        ));
        let f = load(d.to_str().unwrap());
        assert_eq!(f.info, "Folder · 3 items");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn sizes_read_like_finder() {
        assert_eq!(human_size(512), "512 bytes");
        assert_eq!(human_size(1_234_000), "1.2 MB");
    }
}

//! Registers the real distro fonts into egui when the image ships them:
//! Inter for proportional text, JetBrains Mono for monospace. Falls back
//! to egui's bundled defaults when the files aren't there (dev boxes).

use std::{fs, path::PathBuf, sync::Arc};

use egui::{FontData, FontDefinitions, FontFamily};

/// Find a font file under `/usr/share/fonts` whose lowercase filename
/// contains `needle`, preferring `preferred` substrings (e.g. regular
/// weights) when several candidates exist.
fn find_font(needle: &str, preferred: &[&str]) -> Option<PathBuf> {
    let mut hits: Vec<PathBuf> = Vec::new();
    let mut stack = vec![PathBuf::from("/usr/share/fonts")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.contains(needle) && (name.ends_with(".ttf") || name.ends_with(".otf")) {
                hits.push(path);
            }
        }
    }
    hits.sort();
    preferred
        .iter()
        .find_map(|pref| {
            hits.iter()
                .find(|p| p.to_string_lossy().to_lowercase().contains(pref))
        })
        .or_else(|| hits.first())
        .cloned()
}

fn load(needle: &str, preferred: &[&str]) -> Option<FontData> {
    let path = find_font(needle, preferred)?;
    let bytes = fs::read(&path).ok()?;
    tracing::info!(?path, "uitk: registered font");
    Some(FontData::from_owned(bytes))
}

/// Insert the distro faces ahead of egui's bundled fonts — bundled fonts
/// stay in each family as the glyph fallback.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    if let Some(inter) = load("inter", &["regular", "variable", "text", "display"]) {
        fonts.font_data.insert("Inter".to_owned(), Arc::new(inter));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "Inter".to_owned());
    }
    if let Some(mono) = load("jetbrainsmono", &["regular", "medium", "variable"]) {
        fonts
            .font_data
            .insert("JetBrains Mono".to_owned(), Arc::new(mono));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "JetBrains Mono".to_owned());
    }

    ctx.set_fonts(fonts);
}

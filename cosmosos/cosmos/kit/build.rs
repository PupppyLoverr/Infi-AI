//! Rasterize `icons/*.svg` (24px grid, 1.5px stroke, white) to 2× PNGs
//! in OUT_DIR and generate the `Icon` enum that embeds them.

use std::fmt::Write;
use std::path::Path;

const SCALE: f32 = 2.0;

fn camel(name: &str) -> String {
    name.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn main() {
    println!("cargo:rerun-if-changed=icons");
    let out = std::env::var("OUT_DIR").unwrap();
    let mut names: Vec<String> = std::fs::read_dir("icons")
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "svg").then(|| p.file_stem()?.to_str().map(str::to_string))?
        })
        .collect();
    names.sort();
    let opts = resvg::usvg::Options::default();
    for n in &names {
        let data = std::fs::read(format!("icons/{n}.svg")).unwrap();
        let tree = resvg::usvg::Tree::from_data(&data, &opts)
            .unwrap_or_else(|e| panic!("icons/{n}.svg: {e}"));
        let px = (24.0 * SCALE) as u32;
        let mut pm = resvg::tiny_skia::Pixmap::new(px, px).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(SCALE, SCALE),
            &mut pm.as_mut(),
        );
        std::fs::write(
            Path::new(&out).join(format!("{n}.png")),
            pm.encode_png().unwrap(),
        )
        .unwrap();
    }
    let mut src = String::from(
        "/// The kit's original 1.5px-stroke icon set (kit/icons/*.svg).\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum Icon {\n",
    );
    for n in &names {
        writeln!(src, "    {},", camel(n)).unwrap();
    }
    src.push_str("}\n\nimpl Icon {\n    pub const ALL: &'static [Icon] = &[\n");
    for n in &names {
        writeln!(src, "        Icon::{},", camel(n)).unwrap();
    }
    src.push_str("    ];\n\n    pub fn name(self) -> &'static str {\n        match self {\n");
    for n in &names {
        writeln!(src, "            Icon::{} => \"{n}\",", camel(n)).unwrap();
    }
    src.push_str("        }\n    }\n\n    /// The build-time 2× PNG.\n    pub fn png(self) -> &'static [u8] {\n        match self {\n");
    for n in &names {
        writeln!(
            src,
            "            Icon::{} => include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{n}.png\")),",
            camel(n)
        )
        .unwrap();
    }
    src.push_str("        }\n    }\n}\n");
    std::fs::write(Path::new(&out).join("icons_gen.rs"), src).unwrap();
}

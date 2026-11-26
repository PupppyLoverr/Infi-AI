//! Client side of `solyra_glass_v1` (comp/resources/solyra-glass-v1.xml):
//! tells solyra-comp the exact rounded-rectangle shapes of the glass on a
//! panel surface, applied atomically with the surface commit.

#![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#![allow(non_upper_case_globals, non_snake_case, unused_imports, clippy::all)]

pub mod client {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../../comp/resources/solyra-glass-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("../../comp/resources/solyra-glass-v1.xml");
}

pub use client::solyra_glass_manager_v1::SolyraGlassManagerV1;
pub use client::solyra_glass_surface_v1::{Preset, SolyraGlassSurfaceV1};

/// Shapes per surface the compositor accepts.
pub const MAX_SHAPES: usize = 4;

/// One rounded rectangle in surface-local logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub r: f32,
    pub preset: Preset,
}

/// Make a shape list the compositor will accept: drop empty rectangles,
/// keep the first [`MAX_SHAPES`], clamp radii to half the shorter side.
pub fn sanitize(shapes: &[Shape]) -> Vec<Shape> {
    shapes
        .iter()
        .filter(|s| s.w > 0 && s.h > 0)
        .take(MAX_SHAPES)
        .map(|s| Shape {
            r: s.r.max(0.0).min(s.w.min(s.h) as f32 / 2.0),
            ..*s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(w: i32, h: i32, r: f32) -> Shape {
        Shape {
            x: 0,
            y: 0,
            w,
            h,
            r,
            preset: Preset::Clear,
        }
    }

    #[test]
    fn shapes_are_sanitised_before_sending() {
        let out = sanitize(&[s(0, 10, 4.0), s(100, 40, 99.0), s(10, 10, -3.0)]);
        assert_eq!(out.len(), 2, "empty rectangles are dropped");
        assert_eq!(out[0].r, 20.0);
        assert_eq!(out[1].r, 0.0);
        assert_eq!(sanitize(&[s(10, 10, 2.0); 7]).len(), MAX_SHAPES);
    }

    #[test]
    fn the_client_sees_the_same_protocol() {
        use wayland_client::Proxy;
        assert_eq!(
            SolyraGlassSurfaceV1::interface().name,
            "solyra_glass_surface_v1"
        );
        assert_eq!(SolyraGlassManagerV1::interface().version, 1);
    }
}

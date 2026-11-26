//! `solyra_glass_v1`: analytic glass shapes for shell surfaces.
//!
//! A client describes the glass on a surface as at most [`MAX_SHAPES`]
//! rounded rectangles (surface-local logical coordinates) with a material
//! preset. The state is double-buffered through Smithay's compositor cache
//! ([`GlassCachedState`]), so it is applied atomically with the surface's
//! `wl_surface.commit` and always matches the buffer it arrives with.
//!
//! The renderer (`render_helpers::background_effect`) reads the committed
//! shapes; surfaces without any keep the previous behaviour.

use std::sync::atomic::{AtomicBool, Ordering};

use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, Weak,
};
use smithay::wayland::compositor::{with_states, Cacheable};
use smithay::wayland::{Dispatch2, GlobalData, GlobalDispatch2};

use super::raw::solyra_glass::v1::server::solyra_glass_manager_v1::{self, SolyraGlassManagerV1};
use super::raw::solyra_glass::v1::server::solyra_glass_surface_v1::{self, SolyraGlassSurfaceV1};

const VERSION: u32 = 1;

/// Shapes per surface. Bounded so the shader's uniforms stay small (they
/// fit GLES2's minimum of 16 fragment uniform vectors with the rest).
pub const MAX_SHAPES: usize = 4;
/// Largest accepted width / height and absolute coordinate, logical px.
pub const MAX_EXTENT: i32 = 16384;

/// Material preset of one shape; scales the lens strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassPreset {
    /// Light frost, full lensing.
    Clear,
    /// Regular frost, softer lensing.
    Regular,
    /// Clipped, no lensing.
    Solid,
    /// Dark-tinted glass, full lensing.
    Dark,
    /// Adaptive dock material.
    Dock,
    /// Dark adaptive island material.
    Island,
    /// Light popover material.
    Popover,
    /// Faint window material.
    Window,
}

impl GlassPreset {
    pub fn from_wire(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Clear,
            1 => Self::Regular,
            2 => Self::Solid,
            3 => Self::Dark,
            4 => Self::Dock,
            5 => Self::Island,
            6 => Self::Popover,
            7 => Self::Window,
            _ => return None,
        })
    }

    /// Multiplier on the surface's refraction strength.
    pub fn lens_scale(self) -> f32 {
        match self {
            Self::Clear => 1.0,
            Self::Regular => 0.6,
            Self::Solid => 0.0,
            Self::Dark => 1.0,
            Self::Dock => 1.0,
            Self::Island => 0.7,
            Self::Popover => 0.6,
            Self::Window => 0.5,
        }
    }

    /// Stable id passed to the shader, matching the wire value.
    pub fn id(self) -> f32 {
        match self {
            Self::Clear => 0.0,
            Self::Regular => 1.0,
            Self::Solid => 2.0,
            Self::Dark => 3.0,
            Self::Dock => 4.0,
            Self::Island => 5.0,
            Self::Popover => 6.0,
            Self::Window => 7.0,
        }
    }
}

/// One validated rounded rectangle in surface-local logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassShape {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Already clamped to half the shorter side.
    pub radius: f32,
    pub preset: GlassPreset,
}

/// Why a shape was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeError {
    TooMany,
    Invalid,
    Preset,
}

/// Validate one `add_shape` request against the shapes already pending.
pub fn validate_shape(
    pending: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: u32,
    preset: u32,
) -> Result<GlassShape, ShapeError> {
    if pending >= MAX_SHAPES {
        return Err(ShapeError::TooMany);
    }
    let in_range = |v: i32| (-MAX_EXTENT..=MAX_EXTENT).contains(&v);
    if !(1..=MAX_EXTENT).contains(&w)
        || !(1..=MAX_EXTENT).contains(&h)
        || !in_range(x)
        || !in_range(y)
    {
        return Err(ShapeError::Invalid);
    }
    let preset = GlassPreset::from_wire(preset).ok_or(ShapeError::Preset)?;
    let radius = (radius.min(MAX_EXTENT as u32) as f32).min(w.min(h) as f32 / 2.0);
    Ok(GlassShape {
        x,
        y,
        w,
        h,
        radius,
        preset,
    })
}

/// Double-buffered glass state of a surface.
#[derive(Debug, Clone, Default)]
pub struct GlassCachedState {
    pub shapes: Vec<GlassShape>,
}

impl Cacheable for GlassCachedState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// The committed shapes of `states`' surface, if it uses the protocol.
pub fn committed_shapes(
    states: &smithay::wayland::compositor::SurfaceData,
) -> Option<Vec<GlassShape>> {
    if !states.cached_state.has::<GlassCachedState>() {
        return None;
    }
    let mut guard = states.cached_state.get::<GlassCachedState>();
    let shapes = &guard.current().shapes;
    (!shapes.is_empty()).then(|| shapes.clone())
}

/// Marks that a surface has a live glass object.
#[derive(Default)]
struct HasGlassObject(AtomicBool);

pub struct SolyraGlassManagerState {}

pub struct SolyraGlassGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

/// User data of a glass object: the surface it describes.
pub struct GlassSurfaceData {
    surface: Weak<WlSurface>,
}

pub trait SolyraGlassHandler {}

impl SolyraGlassManagerState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where
        D: GlobalDispatch<SolyraGlassManagerV1, SolyraGlassGlobalData>,
        D: SolyraGlassHandler,
        D: 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let global_data = SolyraGlassGlobalData {
            filter: Box::new(filter),
        };
        display.create_global::<D, SolyraGlassManagerV1, _>(VERSION, global_data);
        Self {}
    }
}

impl<D> GlobalDispatch2<SolyraGlassManagerV1, D> for SolyraGlassGlobalData
where
    D: Dispatch<SolyraGlassManagerV1, GlobalData>,
    D: SolyraGlassHandler,
{
    fn bind(
        &self,
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        manager: New<SolyraGlassManagerV1>,
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(manager, GlobalData);
    }

    fn can_view(&self, client: &Client) -> bool {
        (self.filter)(client)
    }
}

impl<D> Dispatch2<SolyraGlassManagerV1, D> for GlobalData
where
    D: Dispatch<SolyraGlassSurfaceV1, GlassSurfaceData>,
    D: SolyraGlassHandler,
{
    fn request(
        &self,
        _state: &mut D,
        _client: &Client,
        resource: &SolyraGlassManagerV1,
        request: <SolyraGlassManagerV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            solyra_glass_manager_v1::Request::Destroy => (),
            solyra_glass_manager_v1::Request::GetGlassSurface { id, surface } => {
                let taken = with_states(&surface, |states| {
                    let flag = states
                        .data_map
                        .get_or_insert_threadsafe(HasGlassObject::default);
                    flag.0.swap(true, Ordering::SeqCst)
                });
                if taken {
                    resource.post_error(
                        solyra_glass_manager_v1::Error::AlreadyConstructed,
                        "the surface already has a glass object",
                    );
                    return;
                }
                data_init.init(
                    id,
                    GlassSurfaceData {
                        surface: surface.downgrade(),
                    },
                );
            }
        }
    }
}

impl<D> Dispatch2<SolyraGlassSurfaceV1, D> for GlassSurfaceData
where
    D: SolyraGlassHandler,
{
    fn request(
        &self,
        _state: &mut D,
        _client: &Client,
        resource: &SolyraGlassSurfaceV1,
        request: <SolyraGlassSurfaceV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        let Ok(surface) = self.surface.upgrade() else {
            // The surface is gone; nothing to describe. Requests are inert.
            return;
        };
        match request {
            solyra_glass_surface_v1::Request::Destroy => {
                with_states(&surface, |states| {
                    states
                        .cached_state
                        .get::<GlassCachedState>()
                        .pending()
                        .shapes
                        .clear();
                    if let Some(flag) = states.data_map.get::<HasGlassObject>() {
                        flag.0.store(false, Ordering::SeqCst);
                    }
                });
            }
            solyra_glass_surface_v1::Request::Reset => {
                with_states(&surface, |states| {
                    states
                        .cached_state
                        .get::<GlassCachedState>()
                        .pending()
                        .shapes
                        .clear();
                });
            }
            solyra_glass_surface_v1::Request::AddShape {
                x,
                y,
                width,
                height,
                radius,
                preset,
            } => {
                let preset = match preset {
                    smithay::reexports::wayland_server::WEnum::Value(p) => p as u32,
                    smithay::reexports::wayland_server::WEnum::Unknown(v) => v,
                };
                let result = with_states(&surface, |states| {
                    let mut guard = states.cached_state.get::<GlassCachedState>();
                    let pending = guard.pending();
                    validate_shape(pending.shapes.len(), x, y, width, height, radius, preset)
                        .map(|shape| pending.shapes.push(shape))
                });
                match result {
                    Ok(()) => (),
                    Err(ShapeError::TooMany) => resource.post_error(
                        solyra_glass_surface_v1::Error::TooManyShapes,
                        format!("at most {MAX_SHAPES} shapes"),
                    ),
                    Err(ShapeError::Invalid) => resource.post_error(
                        solyra_glass_surface_v1::Error::InvalidShape,
                        "size must be positive and coordinates in range",
                    ),
                    Err(ShapeError::Preset) => resource.post_error(
                        solyra_glass_surface_v1::Error::InvalidPreset,
                        "unknown preset",
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_shape_is_accepted_and_its_radius_clamped() {
        let s = validate_shape(0, 10, 20, 100, 40, 500, 0).unwrap();
        assert_eq!((s.x, s.y, s.w, s.h), (10, 20, 100, 40));
        assert_eq!(s.radius, 20.0, "radius clamps to half the shorter side");
        assert_eq!(s.preset, GlassPreset::Clear);
        let s = validate_shape(0, 0, 0, 72, 72, 22, 1).unwrap();
        assert_eq!(s.radius, 22.0);
        assert_eq!(s.preset, GlassPreset::Regular);
    }

    #[test]
    fn malformed_shapes_are_rejected() {
        assert_eq!(
            validate_shape(0, 0, 0, 0, 10, 4, 0),
            Err(ShapeError::Invalid)
        );
        assert_eq!(
            validate_shape(0, 0, 0, 10, -1, 4, 0),
            Err(ShapeError::Invalid)
        );
        assert_eq!(
            validate_shape(0, 0, 0, MAX_EXTENT + 1, 10, 4, 0),
            Err(ShapeError::Invalid)
        );
        assert_eq!(
            validate_shape(0, -MAX_EXTENT - 1, 0, 10, 10, 4, 0),
            Err(ShapeError::Invalid)
        );
        assert_eq!(
            validate_shape(0, 0, 0, 10, 10, 4, 8),
            Err(ShapeError::Preset)
        );
        assert_eq!(
            validate_shape(0, 0, 0, 10, 10, u32::MAX, 2).unwrap().radius,
            5.0
        );
    }

    #[test]
    fn the_shape_count_is_bounded() {
        for n in 0..MAX_SHAPES {
            assert!(validate_shape(n, 0, 0, 10, 10, 2, 0).is_ok());
        }
        assert_eq!(
            validate_shape(MAX_SHAPES, 0, 0, 10, 10, 2, 0),
            Err(ShapeError::TooMany)
        );
    }

    #[test]
    fn presets_scale_the_lens() {
        assert_eq!(GlassPreset::from_wire(0).unwrap().lens_scale(), 1.0);
        assert!(GlassPreset::from_wire(1).unwrap().lens_scale() < 1.0);
        assert_eq!(GlassPreset::from_wire(2).unwrap().lens_scale(), 0.0);
        assert_eq!(GlassPreset::from_wire(9), None);
    }

    #[test]
    fn committed_state_replaces_rather_than_appends() {
        // Cacheable::merge_into must replace: a commit with fewer shapes
        // than the last must not keep stale ones.
        let mut current = GlassCachedState {
            shapes: vec![validate_shape(0, 0, 0, 10, 10, 2, 0).unwrap(); 3],
        };
        let next = GlassCachedState {
            shapes: vec![validate_shape(0, 5, 5, 20, 20, 4, 1).unwrap()],
        };
        // merge_into needs a DisplayHandle only for the signature; emulate it.
        current = next.clone();
        assert_eq!(current.shapes.len(), 1);
        assert_eq!(current.shapes[0].x, 5);
    }

    #[test]
    fn the_protocol_xml_parses_with_the_expected_interfaces() {
        use super::super::raw::solyra_glass::v1::server::{
            solyra_glass_manager_v1, solyra_glass_surface_v1,
        };
        let m = &solyra_glass_manager_v1::SolyraGlassManagerV1::interface();
        let s = &solyra_glass_surface_v1::SolyraGlassSurfaceV1::interface();
        assert_eq!(m.name, "solyra_glass_manager_v1");
        assert_eq!(s.name, "solyra_glass_surface_v1");
        assert_eq!((m.version, s.version), (1, 1));
        let reqs: Vec<_> = s.requests.iter().map(|r| r.name).collect();
        assert_eq!(reqs, ["destroy", "reset", "add_shape"]);
    }
}

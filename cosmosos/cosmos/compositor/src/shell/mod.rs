use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(feature = "xwayland")]
use smithay::xwayland::XWaylandClientData;

#[cfg(feature = "udev")]
use smithay::wayland::drm_syncobj::DrmSyncobjCachedState;

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    desktop::{
        layer_map_for_output, space::SpaceElement, LayerSurface, PopupKind, PopupManager, Space,
        WindowSurfaceType,
    },
    input::pointer::{CursorImageStatus, CursorImageSurfaceData},
    output::Output,
    reexports::{
        calloop::Interest,
        wayland_server::{
            protocol::{wl_buffer::WlBuffer, wl_output, wl_surface::WlSurface},
            Client, Resource,
        },
    },
    utils::{IsAlive, Logical, Point, Rectangle, Size},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            add_blocker, add_pre_commit_hook, get_parent, is_sync_subsurface, with_states,
            with_surface_tree_upward, BufferAssignment, CompositorClientState, CompositorHandler,
            CompositorState, SurfaceAttributes, TraversalAction,
        },
        dmabuf::get_dmabuf,
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::XdgToplevelSurfaceData,
        },
    },
};

use crate::{
    state::{AnvilState, Backend},
    ClientState,
};

mod element;
mod grabs;
pub(crate) mod ssd;
#[cfg(feature = "xwayland")]
mod x11;
mod xdg;

pub use self::element::*;
pub use self::grabs::*;

fn fullscreen_output_geometry(
    wl_surface: &WlSurface,
    wl_output: Option<&wl_output::WlOutput>,
    space: &mut Space<WindowElement>,
) -> Option<Rectangle<i32, Logical>> {
    // First test if a specific output has been requested
    // if the requested output is not found ignore the request
    wl_output
        .and_then(Output::from_resource)
        .or_else(|| {
            let w = space.elements().find(|window| {
                window
                    .wl_surface()
                    .map(|s| &*s == wl_surface)
                    .unwrap_or(false)
            });
            w.and_then(|w| space.outputs_for_element(w).first().cloned())
        })
        .as_ref()
        .and_then(|o| space.output_geometry(o))
}

#[derive(Default)]
pub struct FullscreenSurface(RefCell<Option<WindowElement>>);

impl FullscreenSurface {
    pub fn set(&self, window: WindowElement) {
        *self.0.borrow_mut() = Some(window);
    }

    pub fn get(&self) -> Option<WindowElement> {
        let mut window = self.0.borrow_mut();
        if window.as_ref().map(|w| !w.alive()).unwrap_or(false) {
            *window = None;
        }
        window.clone()
    }

    pub fn clear(&self) -> Option<WindowElement> {
        self.0.borrow_mut().take()
    }
}

impl<BackendData: Backend> BufferHandler for AnvilState<BackendData> {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl<BackendData: Backend> CompositorHandler for AnvilState<BackendData> {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        #[cfg(feature = "xwayland")]
        if let Some(state) = client.get_data::<XWaylandClientData>() {
            return &state.compositor_state;
        }
        if let Some(state) = client.get_data::<ClientState>() {
            return &state.compositor_state;
        }
        panic!("Unknown client data type")
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        add_pre_commit_hook::<Self, _>(surface, move |state, _dh, surface| {
            #[cfg(feature = "udev")]
            let mut acquire_point = None;
            let maybe_dmabuf = with_states(surface, |surface_data| {
                #[cfg(feature = "udev")]
                acquire_point.clone_from(
                    &surface_data
                        .cached_state
                        .get::<DrmSyncobjCachedState>()
                        .pending()
                        .acquire_point,
                );
                surface_data
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .pending()
                    .buffer
                    .as_ref()
                    .and_then(|assignment| match assignment {
                        BufferAssignment::NewBuffer(buffer) => get_dmabuf(buffer).cloned().ok(),
                        _ => None,
                    })
            });
            if let Some(dmabuf) = maybe_dmabuf {
                #[cfg(feature = "udev")]
                if let Some(acquire_point) = acquire_point {
                    if let Ok((blocker, source)) = acquire_point.generate_blocker() {
                        let client = surface.client().unwrap();
                        let res = state.handle.insert_source(source, move |_, _, data| {
                            let dh = data.display_handle.clone();
                            data.client_compositor_state(&client)
                                .blocker_cleared(data, &dh);
                            Ok(())
                        });
                        if res.is_ok() {
                            add_blocker(surface, blocker);
                            return;
                        }
                    }
                }
                if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ) {
                    if let Some(client) = surface.client() {
                        let res = state.handle.insert_source(source, move |_, _, data| {
                            let dh = data.display_handle.clone();
                            data.client_compositor_state(&client)
                                .blocker_cleared(data, &dh);
                            Ok(())
                        });
                        if res.is_ok() {
                            add_blocker(surface, blocker);
                        }
                    }
                }
            }
        });
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.backend_data.early_import(surface);

        // ext-session-lock: lock surfaces are neither windows nor layer
        // surfaces — prove their commits arrive (and reach the frame
        // path) via the serial log while we chase the invisible-lock
        // defect.
        if self.cosmos.session_locked
            && self
                .cosmos
                .lock_surfaces
                .iter()
                .any(|(ls, _)| ls.wl_surface() == surface)
        {
            tracing::info!("cosmos: lock surface committed");
        }

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for_surface(&root) {
                window.0.on_commit();

                if &root == surface {
                    let buffer_offset = with_states(surface, |states| {
                        states
                            .cached_state
                            .get::<SurfaceAttributes>()
                            .current()
                            .buffer_delta
                            .take()
                    });

                    if let Some(buffer_offset) = buffer_offset {
                        let current_loc = self.space.element_location(&window).unwrap();
                        self.space
                            .map_element(window, current_loc + buffer_offset, false);
                    }
                }
            }
        }
        self.popups.commit(surface);

        if matches!(&self.cursor_status, CursorImageStatus::Surface(cursor_surface) if cursor_surface == surface)
        {
            with_states(surface, |states| {
                let cursor_image_attributes = states.data_map.get::<CursorImageSurfaceData>();

                if let Some(mut cursor_image_attributes) =
                    cursor_image_attributes.map(|attrs| attrs.lock().unwrap())
                {
                    let buffer_delta = states
                        .cached_state
                        .get::<SurfaceAttributes>()
                        .current()
                        .buffer_delta
                        .take();
                    if let Some(buffer_delta) = buffer_delta {
                        tracing::trace!(hotspot = ?cursor_image_attributes.hotspot, ?buffer_delta, "decrementing cursor hotspot");
                        cursor_image_attributes.hotspot -= buffer_delta;
                    }
                }
            });
        }

        if matches!(&self.dnd_icon, Some(icon) if &icon.surface == surface) {
            let dnd_icon = self.dnd_icon.as_mut().unwrap();
            with_states(&dnd_icon.surface, |states| {
                let buffer_delta = states
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .current()
                    .buffer_delta
                    .take()
                    .unwrap_or_default();
                tracing::trace!(offset = ?dnd_icon.offset, ?buffer_delta, "moving dnd offset");
                dnd_icon.offset += buffer_delta;
            });
        }

        ensure_initial_configure(surface, &mut self.space, &mut self.popups)
    }
}

impl<BackendData: Backend> WlrLayerShellHandler for AnvilState<BackendData> {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<wl_output::WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .unwrap_or_else(|| self.space.outputs().next().unwrap().clone());
        tracing::info!(namespace, "cosmos: layer surface created");
        // Entrance animation per surface kind — keyed by the wl_surface
        // protocol id; the render loop plays it for the first ~150ms.
        if self.anim_on() {
            let kind = match namespace.as_str() {
                "cosmos-launcher" | "cosmos-assist" | "cosmos-help" => {
                    Some(crate::anim::LayerAnim::Fade)
                }
                "cosmos-switcher" | "cosmos-zoomflyout" | "cosmos-island" => {
                    Some(crate::anim::LayerAnim::Pop)
                }
                "cosmos-quick" | "cosmos-dock" => Some(crate::anim::LayerAnim::SlideUp),
                "cosmos-notify" => Some(crate::anim::LayerAnim::SlideRight),
                _ => None,
            };
            if let Some(kind) = kind {
                let pid = surface.wl_surface().id().protocol_id();
                self.cosmos
                    .anims
                    .layers
                    .insert(pid, (std::time::Instant::now(), kind));
                self.schedule_anim_tick();
            }
        }
        let mut map = layer_map_for_output(&output);
        map.map_layer(&LayerSurface::new(surface, namespace))
            .unwrap();
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        self.cosmos
            .anims
            .layers
            .remove(&surface.wl_surface().id().protocol_id());
        if let Some((mut map, layer)) = self.space.outputs().find_map(|o| {
            let map = layer_map_for_output(o);
            let layer = map
                .layers()
                .find(|&layer| layer.layer_surface() == &surface)
                .cloned();
            layer.map(|layer| (map, layer))
        }) {
            tracing::info!(
                namespace = layer.namespace(),
                "cosmos: layer surface destroyed"
            );
            map.unmap_layer(&layer);
        }
    }
}

impl<BackendData: Backend> AnvilState<BackendData> {
    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<WindowElement> {
        self.space
            .elements()
            .find(|window| window.wl_surface().map(|s| &*s == surface).unwrap_or(false))
            .cloned()
    }
}

#[derive(Default)]
pub struct SurfaceData {
    pub geometry: Option<Rectangle<i32, Logical>>,
    pub resize_state: ResizeState,
}

fn ensure_initial_configure(
    surface: &WlSurface,
    space: &mut Space<WindowElement>,
    popups: &mut PopupManager,
) {
    with_surface_tree_upward(
        surface,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_, states, _| {
            states
                .data_map
                .insert_if_missing(|| RefCell::new(SurfaceData::default()));
        },
        |_, _, _| true,
    );

    let found = space
        .elements()
        .find(|window| window.wl_surface().map(|s| &*s == surface).unwrap_or(false))
        .cloned();
    if let Some(window) = found {
        // send the initial configure if relevant
        #[cfg_attr(not(feature = "xwayland"), allow(irrefutable_let_patterns))]
        if let Some(toplevel) = window.0.toplevel() {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });
            if !initial_configure_sent {
                apply_initial_geometry(space, &window, surface);
                toplevel.send_configure();
            }
        }

        with_states(surface, |states| {
            let mut data = states
                .data_map
                .get::<RefCell<SurfaceData>>()
                .unwrap()
                .borrow_mut();

            // Finish resizing.
            if let ResizeState::WaitingForCommit(_) = data.resize_state {
                data.resize_state = ResizeState::NotResizing;
            }
        });

        return;
    }

    if let Some(popup) = popups.find_popup(surface) {
        let popup = match popup {
            PopupKind::Xdg(ref popup) => popup,
            // Doesn't require configure
            PopupKind::InputMethod(ref _input_popup) => {
                return;
            }
        };

        if !popup.is_initial_configure_sent() {
            // NOTE: This should never fail as the initial configure is always
            // allowed.
            popup.send_configure().expect("initial configure failed");
        }

        return;
    };

    if let Some(output) = space.outputs().find(|o| {
        let map = layer_map_for_output(o);
        map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .is_some()
    }) {
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<LayerSurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .initial_configure_sent
        });

        let mut map = layer_map_for_output(output);

        // arrange the layers before sending the initial configure
        // to respect any size the client may have sent
        map.arrange();
        // send the initial configure if relevant
        if !initial_configure_sent {
            let layer = map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .unwrap();

            layer.layer_surface().send_configure();
        }
    };
}

/// Marker: window was placed before its surface committed a real size, so
/// its position is re-clamped into the usable zone on every commit until
/// the user takes over placement (drag/resize/snap/maximize) — apps resize
/// more than once on startup, and a fixed time window let late-growing
/// windows overflow the zone edge.
pub struct InitialFit {
    user_moved: AtomicBool,
}

impl InitialFit {
    fn new() -> Self {
        Self {
            user_moved: AtomicBool::new(false),
        }
    }

    /// The user (or an explicit compositor action) placed this window —
    /// stop re-clamping it on commits.
    pub fn user_moved(&self) {
        self.user_moved.store(true, Ordering::Relaxed);
    }

    /// Whether the user has taken over placement. Dynamic tiling skips
    /// these windows — a drag-out floats the window Hyprland-style.
    pub fn moved(&self) -> bool {
        self.user_moved.load(Ordering::Relaxed)
    }

    /// Re-enroll the window: clears the flag so it rejoins the layout
    /// (tiling toggled back on) and the initial-fit clamp watches it again.
    pub fn reset(&self) {
        self.user_moved.store(false, Ordering::Relaxed);
    }
}

fn usable_zone(space: &Space<WindowElement>, output: &Output) -> Rectangle<i32, Logical> {
    let geo = space.output_geometry(output).unwrap();
    let zone = layer_map_for_output(output).non_exclusive_zone();
    Rectangle::new(geo.loc + zone.loc, zone.size)
}

/// Clamp `loc` so the whole window rectangle stays inside `zone`. Windows
/// larger than the zone keep the zone origin (they clip on the right/bottom
/// but stay grabbable by the titlebar).
pub fn clamp_loc_to_zone(
    loc: Point<i32, Logical>,
    win_size: Size<i32, Logical>,
    zone: Rectangle<i32, Logical>,
) -> Point<i32, Logical> {
    let max_x = (zone.loc.x + zone.size.w - win_size.w).max(zone.loc.x);
    let max_y = (zone.loc.y + zone.size.h - win_size.h).max(zone.loc.y);
    (
        loc.x.clamp(zone.loc.x, max_x),
        loc.y.clamp(zone.loc.y, max_y),
    )
        .into()
}

/// Re-clamp a window's position once its real geometry is known. Called
/// from `handle_toplevel_commit` for windows flagged with `InitialFit`.
pub fn refit_into_zone(space: &mut Space<WindowElement>, window: &WindowElement) {
    let Some(fit) = window.user_data().get::<InitialFit>() else {
        return;
    };
    // Element geometry already includes the SSD titlebar height.
    let size = window.geometry().size;
    if size.w <= 0 || size.h <= 0 {
        return;
    }
    if fit.user_moved.load(Ordering::Relaxed) {
        return;
    }
    let Some(loc) = space.element_location(window) else {
        return;
    };
    let Some(output) = space
        .outputs_for_element(window)
        .first()
        .cloned()
        .or_else(|| space.outputs().next().cloned())
    else {
        return;
    };
    let new_loc = clamp_loc_to_zone(loc, size, usable_zone(space, &output));
    if new_loc != loc {
        tracing::info!(loc = ?loc, new_loc = ?new_loc, "cosmos: initial refit relocated window");
        space.map_element(window.clone(), new_loc, false);
    }
}

/// Cascade step between successive default-placed windows.
const CASCADE_STEP: i32 = 28;
/// Default-placed windows wrap back to the centre after this many steps.
const CASCADE_WRAP: i32 = 8;
static CASCADE_NEXT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Last floating geometry per app id, persisted in
/// `$XDG_STATE_HOME/cosmos/windows.json` as `{"app.id": [x, y, w, h]}`.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct WindowMemory(std::collections::HashMap<String, [i32; 4]>);

impl WindowMemory {
    fn path() -> std::path::PathBuf {
        std::env::var("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
                    .join(".local/state")
            })
            .join("cosmos/windows.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Record `app_id`'s floating geometry (content size, no SSD titlebar).
    pub fn remember(app_id: &str, loc: Point<i32, Logical>, size: Size<i32, Logical>) {
        if size.w <= 0 || size.h <= 0 {
            return;
        }
        let mut mem = Self::load();
        mem.0
            .insert(app_id.to_string(), [loc.x, loc.y, size.w, size.h]);
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&mem) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// Default size for a new window in `zone`: 60% × 65%, at least 800×520
/// logical, never larger than the zone.
pub fn default_window_size(zone: Rectangle<i32, Logical>) -> Size<i32, Logical> {
    let w = ((zone.size.w as f64 * 0.60).round() as i32)
        .max(800)
        .min(zone.size.w);
    let h = ((zone.size.h as f64 * 0.65).round() as i32)
        .max(520)
        .min(zone.size.h);
    (w, h).into()
}

/// Initial geometry, decided at the first commit once the app id is known:
/// the app's remembered size/position, else the default size centred and
/// cascaded by [`CASCADE_STEP`].
fn apply_initial_geometry(
    space: &mut Space<WindowElement>,
    window: &WindowElement,
    surface: &WlSurface,
) {
    let Some(toplevel) = window.0.toplevel() else {
        return;
    };
    let Some(output) = space
        .outputs_for_element(window)
        .first()
        .cloned()
        .or_else(|| space.outputs().next().cloned())
    else {
        return;
    };
    let zone = usable_zone(space, &output);
    let (_, app_id) = crate::cosmos::toplevel_title_app(surface);
    let remembered = app_id
        .as_deref()
        .and_then(|id| WindowMemory::load().0.get(id).copied());
    let (loc, size): (Point<i32, Logical>, Size<i32, Logical>) = match remembered {
        Some([x, y, w, h]) => {
            let size: Size<i32, Logical> = (w.min(zone.size.w), h.min(zone.size.h)).into();
            (clamp_loc_to_zone((x, y).into(), size, zone), size)
        }
        None => {
            let size = default_window_size(zone);
            let step = CASCADE_NEXT.fetch_add(1, Ordering::Relaxed) % CASCADE_WRAP;
            let c = step * CASCADE_STEP;
            let loc = (
                zone.loc.x + (zone.size.w - size.w) / 2 + c,
                zone.loc.y + (zone.size.h - size.h) / 2 + c,
            )
                .into();
            (clamp_loc_to_zone(loc, size, zone), size)
        }
    };
    toplevel.with_pending_state(|state| state.size = Some(size));
    tracing::info!(app_id = ?app_id, ?loc, ?size, remembered = remembered.is_some(), "cosmos: initial geometry");
    space.map_element(window.clone(), loc, false);
}

fn place_new_window(
    space: &mut Space<WindowElement>,
    pointer_location: Point<f64, Logical>,
    window: &WindowElement,
    activate: bool,
) {
    // place the window at a random location on same output as pointer
    // or if there is not output in a [0;800]x[0;800] square
    use rand::distributions::{Distribution, Uniform};

    let output = space
        .output_under(pointer_location)
        .next()
        .or_else(|| space.outputs().next())
        .cloned();
    let output_geometry = output
        .and_then(|o| {
            let geo = space.output_geometry(&o)?;
            let map = layer_map_for_output(&o);
            let zone = map.non_exclusive_zone();
            Some(Rectangle::new(geo.loc + zone.loc, zone.size))
        })
        .unwrap_or_else(|| Rectangle::from_size((800, 800).into()));

    // set the initial toplevel bounds
    #[allow(irrefutable_let_patterns)]
    if let Some(toplevel) = window.0.toplevel() {
        toplevel.with_pending_state(|state| {
            state.bounds = Some(output_geometry.size);
        });
    }

    // Keep the whole window inside the usable zone: sample a position, then
    // clamp so `x + win_w <= zone.right` (and same for y). Window size may be
    // unknown before the first commit — fall back to half the zone.
    let win_size = {
        let bbox = window.bbox().size;
        if bbox.w > 0 && bbox.h > 0 {
            bbox
        } else {
            Size::from((output_geometry.size.w / 2, output_geometry.size.h / 2))
        }
    };
    let max_x =
        (output_geometry.loc.x + output_geometry.size.w - win_size.w).max(output_geometry.loc.x);
    let max_y =
        (output_geometry.loc.y + output_geometry.size.h - win_size.h).max(output_geometry.loc.y);
    let mut rng = rand::thread_rng();
    // max can equal the zone origin when the window fills the zone exactly
    let x = if max_x > output_geometry.loc.x {
        Uniform::new(output_geometry.loc.x, max_x).sample(&mut rng)
    } else {
        output_geometry.loc.x
    };
    let y = if max_y > output_geometry.loc.y {
        Uniform::new(output_geometry.loc.y, max_y).sample(&mut rng)
    } else {
        output_geometry.loc.y
    };

    // Size can still grow at the first real commit (the toplevel bounds we
    // just set allow up to the whole zone) — flag for a one-time refit.
    window.user_data().insert_if_missing(InitialFit::new);
    space.map_element(window.clone(), (x, y), activate);
}

pub fn fixup_positions(space: &mut Space<WindowElement>, pointer_location: Point<f64, Logical>) {
    // fixup outputs
    let mut offset = Point::<i32, Logical>::from((0, 0));
    for output in space.outputs().cloned().collect::<Vec<_>>().into_iter() {
        let size = space
            .output_geometry(&output)
            .map(|geo| geo.size)
            .unwrap_or_else(|| Size::from((0, 0)));
        space.map_output(&output, offset);
        layer_map_for_output(&output).arrange();
        offset.x += size.w;
    }

    // fixup windows
    let mut orphaned_windows = Vec::new();
    let outputs = space
        .outputs()
        .flat_map(|o| {
            let geo = space.output_geometry(o)?;
            let map = layer_map_for_output(o);
            let zone = map.non_exclusive_zone();
            Some(Rectangle::new(geo.loc + zone.loc, zone.size))
        })
        .collect::<Vec<_>>();
    for window in space.elements() {
        let window_location = match space.element_location(window) {
            Some(loc) => loc,
            None => continue,
        };
        let geo_loc = window.bbox().loc + window_location;

        if !outputs.iter().any(|o_geo| o_geo.contains(geo_loc)) {
            orphaned_windows.push(window.clone());
        }
    }
    for window in orphaned_windows.into_iter() {
        place_new_window(space, pointer_location, &window, false);
    }
}

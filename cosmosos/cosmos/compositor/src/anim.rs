//! Cosmos motion system.
//!
//! Transition animations are entirely render-time: the render loop wraps
//! each animating window or layer surface's elements in a rescale/relocate
//! transform for the duration of the animation, and
//! [`crate::state::AnvilState::tick_animations`] completes the deferred
//! unmaps (minimize-out, workspace slide-out) once they expire.
//!
//! Durations follow the macOS idiom — fast enough to never be in the way
//! (130–190ms), eased with a decelerating cubic so motion reads soft, not
//! bouncy. The whole system is disabled by `reduce_motion` in the user
//! config.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Window map-in: fade + zoom from ~94% (macOS sheet feel, subtler).
pub const MAP_IN: Duration = Duration::from_millis(160);
/// Minimize-out: fade + shrink — the window visibly leaves before it unmaps.
pub const MINIMIZE_OUT: Duration = Duration::from_millis(130);
/// Workspace switch: windows of the incoming desktop slide in sideways.
pub const WS_SLIDE: Duration = Duration::from_millis(190);
/// Floating cards: pop/fade/slide entrances.
pub const LAYER_IN: Duration = Duration::from_millis(150);

/// Standard decelerating curve — fast start, soft landing.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Per-window transition currently running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinAnim {
    /// Fade + scale-in at map time.
    MapIn,
    /// Fade + shrink before the window is parked as minimized.
    MinimizeOut,
}

/// Entrance style for a layer-shell card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerAnim {
    /// Fade only — fullscreen overlays (launcher, assist) where scaling
    /// the whole surface would expose edges.
    Fade,
    /// Scale + fade about the card's own center (switcher, popovers).
    Pop,
    /// Slide up from the bottom + fade (quick settings, dock).
    SlideUp,
    /// Slide in from the right edge + fade (notification stack).
    SlideRight,
}

/// In-flight workspace slide. `leaving` holds the window ids of the
/// outgoing desktop; they stay mapped (sliding out) until the animation
/// expires and `tick_animations` parks them.
#[derive(Debug)]
pub struct WsSlide {
    pub start: Instant,
    /// +1 when moving to a higher-numbered workspace (new slides in
    /// from the right), -1 otherwise.
    pub dir: i32,
    pub leaving: Vec<u64>,
}

/// All running animations, keyed the way the render loop can look them up.
#[derive(Debug, Default)]
pub struct Animations {
    /// window id → (start, kind)
    pub windows: HashMap<u64, (Instant, WinAnim)>,
    /// layer `wl_surface` protocol id → (start, kind)
    pub layers: HashMap<u32, (Instant, LayerAnim)>,
    pub ws: Option<WsSlide>,
}

impl Animations {
    /// (alpha, scale) for an animating window at `now`. `None` = no
    /// animating transform — the caller emits the plain element.
    pub fn window_tx(&self, id: u64, now: Instant) -> Option<(f32, f64)> {
        let (start, kind) = self.windows.get(&id)?;
        let dur = match kind {
            WinAnim::MapIn => MAP_IN,
            WinAnim::MinimizeOut => MINIMIZE_OUT,
        };
        let t = (now.duration_since(*start).as_secs_f32() / dur.as_secs_f32()).min(1.0);
        let e = ease_out_cubic(t);
        Some(match kind {
            WinAnim::MapIn => (e, lerp(0.94, 1.0, e)),
            WinAnim::MinimizeOut => (1.0 - e, lerp(1.0, 0.90, e)),
        })
    }

    /// Horizontal slide offset (logical px) for a window during a
    /// workspace transition: outgoing windows slide out, everything else
    /// (the incoming desktop) slides in from the direction edge.
    pub fn ws_dx(&self, id: u64, now: Instant, output_w: i32) -> i32 {
        let Some(anim) = &self.ws else { return 0 };
        let t = (now.duration_since(anim.start).as_secs_f32() / WS_SLIDE.as_secs_f32()).min(1.0);
        let e = ease_out_cubic(t);
        if anim.leaving.contains(&id) {
            -(e * output_w as f32 * anim.dir as f32) as i32
        } else {
            ((1.0 - e) * output_w as f32 * anim.dir as f32) as i32
        }
    }

    /// (alpha, dx, dy, scale) for an animating layer surface at `now`.
    /// `geo` is the layer's logical geometry (dx for SlideRight is a
    /// fraction of its width; Pop scales about its center).
    pub fn layer_tx(
        &self,
        protocol_id: u32,
        now: Instant,
        geo_w: i32,
    ) -> Option<(f32, i32, i32, f64)> {
        let (start, kind) = self.layers.get(&protocol_id)?;
        let t = (now.duration_since(*start).as_secs_f32() / LAYER_IN.as_secs_f32()).min(1.0);
        let e = ease_out_cubic(t);
        Some(match kind {
            LayerAnim::Fade => (e, 0, 0, 1.0),
            LayerAnim::Pop => (e, 0, 0, lerp(0.90, 1.0, e)),
            LayerAnim::SlideUp => (e, 0, ((1.0 - e) * 28.0) as i32, 1.0),
            LayerAnim::SlideRight => (e, ((1.0 - e) * geo_w as f32 * 0.35) as i32, 0, 1.0),
        })
    }

    /// Any animation still inside its duration.
    pub fn any_running(&self, now: Instant) -> bool {
        if let Some(anim) = &self.ws {
            if now.duration_since(anim.start) < WS_SLIDE {
                return true;
            }
        }
        if self
            .windows
            .values()
            .any(|(start, kind)| now.duration_since(*start) < dur_of(*kind))
        {
            return true;
        }
        self.layers
            .values()
            .any(|(start, _)| now.duration_since(*start) < LAYER_IN)
    }

    /// Deferred unmaps that must still complete even if frames stall —
    /// drives the fallback timer, not the render fast path.
    pub fn needs_timer(&self) -> bool {
        self.ws.is_some()
            || self
                .windows
                .values()
                .any(|(_, k)| *k == WinAnim::MinimizeOut)
    }

    /// Window ids whose minimize-out animation has finished — the caller
    /// parks them.
    pub fn expired_minimizes(&self, now: Instant) -> Vec<u64> {
        self.windows
            .iter()
            .filter(|(_, (start, kind))| {
                *kind == WinAnim::MinimizeOut && now.duration_since(*start) >= MINIMIZE_OUT
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Whether the workspace slide has run out.
    pub fn ws_expired(&self, now: Instant) -> bool {
        self.ws
            .as_ref()
            .map(|anim| now.duration_since(anim.start) >= WS_SLIDE)
            .unwrap_or(false)
    }

    /// Drop entries that no longer affect rendering (finished map-ins,
    /// finished layer entrances). Expired minimizes and ws slides are
    /// handled by their completion paths, not pruned here.
    pub fn prune(&mut self, now: Instant) {
        self.windows.retain(|_, (start, kind)| {
            *kind == WinAnim::MinimizeOut || now.duration_since(*start) < dur_of(*kind)
        });
        self.layers
            .retain(|_, (start, _)| now.duration_since(*start) < LAYER_IN);
    }
}

fn dur_of(kind: WinAnim) -> Duration {
    match kind {
        WinAnim::MapIn => MAP_IN,
        WinAnim::MinimizeOut => MINIMIZE_OUT,
    }
}

pub fn lerp(a: f64, b: f64, t: f32) -> f64 {
    a + (b - a) * t as f64
}

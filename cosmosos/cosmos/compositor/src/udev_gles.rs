//! GLES flavour of the tty backend, for machines with a hardware GPU.

use smithay::{
    backend::{
        allocator::{format::FormatSet, gbm::GbmDevice},
        drm::{DrmDeviceFd, DrmNode},
        egl::{context::ContextPriority, EGLDevice, EGLDisplay},
        renderer::{
            gles::{GlesRenderer, GlesTexture},
            multigpu::{gbm::GbmGlesBackend, GpuManager, MultiRenderer},
        },
    },
    reexports::wayland_server::DisplayHandle,
};

#[allow(clippy::duplicate_mod, reason = "one backend body per renderer")]
#[path = "udev_imp.rs"]
mod imp;
pub use imp::*;

pub type Api = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;
pub type OffscreenTarget = GlesTexture;

fn new_api() -> Api {
    GbmGlesBackend::with_context_priority(ContextPriority::High)
}

fn init_gpu(
    gpus: &mut GpuManager<Api>,
    node: DrmNode,
    primary_gpu: DrmNode,
    gbm: &GbmDevice<DrmDeviceFd>,
    _fd: &DrmDeviceFd,
) -> Result<DrmNode, DeviceAddError> {
    // SAFETY: the GBM device is kept alive by the backend for the display's lifetime.
    let display = unsafe { EGLDisplay::new(gbm.clone()).map_err(DeviceAddError::AddNode)? };
    let egl_device = EGLDevice::device_for_display(&display).map_err(DeviceAddError::AddNode)?;
    let render_node =
        egl_device
            .try_get_render_node()
            .ok()
            .flatten()
            .unwrap_or(if egl_device.is_software() {
                primary_gpu
            } else {
                node
            });
    gpus.as_mut()
        .add_node(render_node, gbm.clone())
        .map_err(DeviceAddError::AddNode)?;
    Ok(render_node)
}

fn render_formats(renderer: &mut GlesRenderer) -> FormatSet {
    renderer.egl_context().dmabuf_render_formats().clone()
}

#[cfg_attr(not(feature = "egl"), allow(unused_variables))]
fn bind_wl_display(renderer: &mut MultiRenderer<'_, '_, Api, Api>, dh: &DisplayHandle) {
    #[cfg(feature = "egl")]
    {
        use smithay::backend::renderer::ImportEgl;
        tracing::info!("Trying to initialize EGL Hardware Acceleration");
        match renderer.bind_wl_display(dh) {
            Ok(_) => tracing::info!("EGL hardware-acceleration enabled"),
            Err(err) => tracing::info!(?err, "Failed to initialize EGL hardware-acceleration"),
        }
    }
}

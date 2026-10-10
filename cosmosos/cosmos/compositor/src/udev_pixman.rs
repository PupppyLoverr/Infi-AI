//! Pixman flavour of the tty backend: CPU compositing for machines whose
//! only GL is a software rasterizer, so Mesa's llvmpipe is never loaded.

use smithay::{
    backend::{
        allocator::{dmabuf::Dmabuf, format::FormatSet, gbm::GbmDevice},
        drm::{DrmDeviceFd, DrmNode, NodeType},
        renderer::{
            multigpu::{GpuManager, MultiRenderer},
            pixman::PixmanRenderer,
            Bind,
        },
    },
    reexports::wayland_server::DisplayHandle,
};

use crate::pixman_backend::PixmanBackend;

#[allow(clippy::duplicate_mod, reason = "one backend body per renderer")]
#[path = "udev_imp.rs"]
mod imp;
pub use imp::*;

pub type Api = PixmanBackend;
pub type OffscreenTarget = smithay::reexports::pixman::Image<'static, 'static>;

fn new_api() -> Api {
    PixmanBackend::default()
}

fn init_gpu(
    gpus: &mut GpuManager<Api>,
    node: DrmNode,
    primary_gpu: DrmNode,
    _gbm: &GbmDevice<DrmDeviceFd>,
    fd: &DrmDeviceFd,
) -> Result<DrmNode, DeviceAddError> {
    let render_node = node
        .node_with_type(NodeType::Render)
        .and_then(Result::ok)
        .unwrap_or(primary_gpu);
    gpus.as_mut().add_node(render_node, fd.clone());
    Ok(render_node)
}

fn render_formats(renderer: &mut PixmanRenderer) -> FormatSet {
    Bind::<Dmabuf>::supported_formats(renderer).unwrap_or_default()
}

fn bind_wl_display(_renderer: &mut MultiRenderer<'_, '_, Api, Api>, _dh: &DisplayHandle) {}

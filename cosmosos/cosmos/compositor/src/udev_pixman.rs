//! Pixman flavour of the tty backend: CPU compositing for machines whose
//! only GL is a software rasterizer, so Mesa's llvmpipe is never loaded:
//! no EGL and no GBM, scanout goes through linear dumb buffers.

use smithay::{
    backend::{
        allocator::{
            dmabuf::Dmabuf,
            dumb::{DumbAllocator, DumbBuffer},
            format::FormatSet,
            gbm::GbmDevice,
            Allocator, Fourcc, Modifier,
        },
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
pub type Alloc = DumbAlloc;
pub type Exporter = DrmDeviceFd;
/// No GBM device: creating one loads Mesa's `dri_gbm.so`, and with it llvmpipe.
type Device = ();

/// Linear dumb buffers on the card node, cloneable so outputs on secondary
/// devices can share the primary's allocator.
#[derive(Debug)]
pub struct DumbAlloc {
    fd: DrmDeviceFd,
    inner: DumbAllocator,
}

impl Clone for DumbAlloc {
    fn clone(&self) -> Self {
        Self {
            fd: self.fd.clone(),
            inner: DumbAllocator::new(self.fd.clone()),
        }
    }
}

impl Allocator for DumbAlloc {
    type Buffer = DumbBuffer;
    type Error = <DumbAllocator as Allocator>::Error;

    fn create_buffer(
        &mut self,
        width: u32,
        height: u32,
        fourcc: Fourcc,
        modifiers: &[Modifier],
    ) -> Result<DumbBuffer, Self::Error> {
        self.inner.create_buffer(width, height, fourcc, modifiers)
    }
}

fn open_device(_fd: &DrmDeviceFd) -> Result<Device, DeviceAddError> {
    Ok(())
}

fn new_allocator(_dev: &Device, fd: &DrmDeviceFd) -> Alloc {
    DumbAlloc {
        fd: fd.clone(),
        inner: DumbAllocator::new(fd.clone()),
    }
}

fn new_exporter(_dev: &Device, fd: &DrmDeviceFd, _render_node: Option<DrmNode>) -> Exporter {
    fd.clone()
}

fn cursor_gbm(_dev: Device) -> Option<GbmDevice<DrmDeviceFd>> {
    None
}

fn new_api() -> Api {
    PixmanBackend::default()
}

fn init_gpu(
    gpus: &mut GpuManager<Api>,
    node: DrmNode,
    primary_gpu: DrmNode,
    _dev: &Device,
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
    Bind::<Dmabuf>::supported_formats(renderer)
        .unwrap_or_default()
        .iter()
        .filter(|format| format.modifier == Modifier::Linear)
        .copied()
        .collect()
}

fn bind_wl_display(_renderer: &mut MultiRenderer<'_, '_, Api, Api>, _dh: &DisplayHandle) {}

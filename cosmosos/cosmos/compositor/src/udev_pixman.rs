//! Pixman flavour of the tty backend: CPU compositing for machines whose
//! only GL is a software rasterizer, so Mesa's llvmpipe is never loaded:
//! no EGL and no GBM, scanout goes through linear dumb buffers.

use smithay::{
    backend::{
        allocator::{
            dmabuf::Dmabuf,
            dumb::{DumbAllocator, DumbBuffer},
            format::{get_opaque, FormatSet},
            gbm::GbmDevice,
            Allocator, Format as DrmFormat, Fourcc, Modifier,
        },
        drm::{
            exporter::{ExportBuffer, ExportFramebuffer},
            Framebuffer,
        },
        drm::{DrmDeviceFd, DrmNode, NodeType},
        renderer::{
            multigpu::{GpuManager, MultiRenderer},
            pixman::PixmanRenderer,
            Bind,
        },
    },
    reexports::{
        drm::{
            buffer::PlanarBuffer,
            control::{dumbbuffer, framebuffer, Device as _, FbCmd2Flags},
        },
        wayland_server::DisplayHandle,
    },
};

use crate::pixman_backend::PixmanBackend;

#[allow(clippy::duplicate_mod, reason = "one backend body per renderer")]
#[path = "udev_imp.rs"]
mod imp;
pub use imp::*;

pub type Api = PixmanBackend;
pub type OffscreenTarget = smithay::reexports::pixman::Image<'static, 'static>;
pub type Alloc = DumbAlloc;
pub type Exporter = DumbExporter;
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
    use smithay::reexports::drm::{Device as _, DriverCapability};

    DumbExporter {
        fd: fd.clone(),
        modifiers: fd
            .get_driver_capability(DriverCapability::AddFB2Modifiers)
            .is_ok_and(|cap| cap == 1),
    }
}

/// Adds dumb-buffer framebuffers with ADDFB2, passing the modifier only when
/// the driver supports modifiers. smithay's dumb exporter always passes it,
/// and on drivers without them (virtio-gpu) falls back to a legacy ARGB
/// framebuffer that the opaque primary plane rejects.
#[derive(Debug, Clone)]
pub struct DumbExporter {
    fd: DrmDeviceFd,
    modifiers: bool,
}

#[derive(Debug)]
pub struct DumbFramebuffer {
    fd: DrmDeviceFd,
    handle: framebuffer::Handle,
    format: DrmFormat,
}

impl AsRef<framebuffer::Handle> for DumbFramebuffer {
    fn as_ref(&self) -> &framebuffer::Handle {
        &self.handle
    }
}

impl Framebuffer for DumbFramebuffer {
    fn format(&self) -> DrmFormat {
        self.format
    }
}

impl Drop for DumbFramebuffer {
    fn drop(&mut self) {
        let _ = self.fd.destroy_framebuffer(self.handle);
    }
}

struct DumbPlanes<'a> {
    buffer: &'a dumbbuffer::DumbBuffer,
    code: Fourcc,
    modifier: Option<Modifier>,
}

impl PlanarBuffer for DumbPlanes<'_> {
    fn size(&self) -> (u32, u32) {
        smithay::reexports::drm::buffer::Buffer::size(self.buffer)
    }

    fn format(&self) -> Fourcc {
        self.code
    }

    fn modifier(&self) -> Option<Modifier> {
        self.modifier
    }

    fn pitches(&self) -> [u32; 4] {
        [
            smithay::reexports::drm::buffer::Buffer::pitch(self.buffer),
            0,
            0,
            0,
        ]
    }

    fn handles(&self) -> [Option<smithay::reexports::drm::buffer::Handle>; 4] {
        [
            Some(smithay::reexports::drm::buffer::Buffer::handle(self.buffer)),
            None,
            None,
            None,
        ]
    }

    fn offsets(&self) -> [u32; 4] {
        [0; 4]
    }
}

impl ExportFramebuffer<DumbBuffer> for DumbExporter {
    type Framebuffer = DumbFramebuffer;
    type Error = std::io::Error;

    fn add_framebuffer(
        &self,
        _drm: &DrmDeviceFd,
        buffer: ExportBuffer<'_, DumbBuffer>,
        use_opaque: bool,
    ) -> Result<Option<DumbFramebuffer>, std::io::Error> {
        let ExportBuffer::Allocator(buffer) = buffer else {
            return Ok(None);
        };
        let format = smithay::backend::allocator::Buffer::format(buffer);
        let code = if use_opaque {
            get_opaque(format.code).unwrap_or(format.code)
        } else {
            format.code
        };
        let (modifier, flags) = if self.modifiers {
            (Some(format.modifier), FbCmd2Flags::MODIFIERS)
        } else {
            (None, FbCmd2Flags::empty())
        };
        let planes = DumbPlanes {
            buffer: buffer.handle(),
            code,
            modifier,
        };
        let handle = self.fd.add_planar_framebuffer(&planes, flags)?;
        Ok(Some(DumbFramebuffer {
            fd: self.fd.clone(),
            handle,
            format: DrmFormat {
                code,
                modifier: format.modifier,
            },
        }))
    }

    fn can_add_framebuffer(&self, buffer: &ExportBuffer<'_, DumbBuffer>) -> bool {
        matches!(buffer, ExportBuffer::Allocator(_))
    }
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

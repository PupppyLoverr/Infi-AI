//! Smithay multi-GPU `GraphicsApi` backed by the Pixman CPU renderer.
//! On machines without a real GPU this replaces GLES-on-llvmpipe, so
//! Mesa's LLVM JIT (~70 MB resident) is never loaded.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

use smithay::backend::{
    allocator::{
        dmabuf::{AnyError, Dmabuf, DmabufAllocator},
        dumb::DumbAllocator,
        Allocator,
    },
    drm::{DrmDeviceFd, DrmNode},
    renderer::{
        multigpu::{ApiDevice, GraphicsApi},
        pixman::{PixmanError, PixmanRenderer},
    },
};

#[derive(Debug, Default)]
pub struct PixmanBackend {
    devices: HashMap<DrmNode, DrmDeviceFd>,
    needs_enumeration: AtomicBool,
}

impl PixmanBackend {
    pub fn add_node(&mut self, node: DrmNode, fd: DrmDeviceFd) {
        if self.devices.insert(node, fd).is_none() {
            self.needs_enumeration.store(true, Ordering::SeqCst);
        }
    }

    pub fn remove_node(&mut self, node: &DrmNode) {
        if self.devices.remove(node).is_some() {
            self.needs_enumeration.store(true, Ordering::SeqCst);
        }
    }
}

pub struct PixmanDevice {
    node: DrmNode,
    renderer: PixmanRenderer,
    allocator: Box<dyn Allocator<Buffer = Dmabuf, Error = AnyError>>,
}

impl std::fmt::Debug for PixmanDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PixmanDevice")
            .field("node", &self.node)
            .finish_non_exhaustive()
    }
}

impl ApiDevice for PixmanDevice {
    type Renderer = PixmanRenderer;

    fn renderer(&self) -> &PixmanRenderer {
        &self.renderer
    }
    fn renderer_mut(&mut self) -> &mut PixmanRenderer {
        &mut self.renderer
    }
    fn allocator(&mut self) -> &mut dyn Allocator<Buffer = Dmabuf, Error = AnyError> {
        self.allocator.as_mut()
    }
    fn node(&self) -> &DrmNode {
        &self.node
    }
}

impl GraphicsApi for PixmanBackend {
    type Device = PixmanDevice;
    type Error = PixmanError;

    fn enumerate(&self, list: &mut Vec<PixmanDevice>) -> Result<(), PixmanError> {
        self.needs_enumeration.store(false, Ordering::SeqCst);
        list.retain(|d| self.devices.contains_key(&d.node));
        for (node, fd) in &self.devices {
            if list.iter().any(|d| d.node == *node) {
                continue;
            }
            list.push(PixmanDevice {
                node: *node,
                renderer: PixmanRenderer::new()?,
                allocator: Box::new(DmabufAllocator(DumbAllocator::new(fd.clone()))),
            });
        }
        Ok(())
    }

    fn needs_enumeration(&self) -> bool {
        self.needs_enumeration.load(Ordering::Acquire)
    }

    fn identifier() -> &'static str {
        "pixman"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::backend::renderer::{
        multigpu::MultiRenderer, Bind, ExportMem, ImportDma, ImportMem, ImportMemWl, Offscreen,
        Renderer,
    };

    #[test]
    fn multirenderer_has_compositor_bounds() {
        fn needs<R: Renderer + Bind<Dmabuf> + ImportMem + ImportMemWl + ImportDma + ExportMem>() {}
        needs::<MultiRenderer<'static, 'static, PixmanBackend, PixmanBackend>>();
        fn offscreen<R: Offscreen<smithay::reexports::pixman::Image<'static, 'static>>>() {}
        offscreen::<PixmanRenderer>();
    }
}

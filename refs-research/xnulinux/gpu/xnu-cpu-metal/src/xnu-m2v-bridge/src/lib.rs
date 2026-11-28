// SPDX-License-Identifier: GPL-2.0-or-later
//! C ABI bridge over `metal2vulkan` and `metal2vulkan-runtime` for xnulinux CPU Metal.

use metal2vulkan::bind::SamplerDescriptor;
use metal2vulkan::library::{FunctionConstantValue, FunctionConstantValues};
use metal2vulkan::metal::state::{DepthStencilDescriptor, StencilDescriptor};
use metal2vulkan::metal::{
    BlendFactor, BlendOperation, ColorWriteMask, CompareFunction, CullMode, DataType,
    FunctionOptions, IndexType, PixelFormat, PrimitiveType, RenderStages, ResourceUsage,
    SamplerAddressMode, SamplerMinMagFilter, SamplerMipFilter, StencilOperation, TextureType,
    TextureUsage, VertexFormat, VertexStepFunction, Winding,
};
use metal2vulkan::pipeline::{
    ColorAttachmentDescriptor, ComputePipelineDescriptor, LinkedFunctions,
    RenderPipelineDescriptor, StageSource, VertexAttributeDescriptor,
    VertexBufferLayoutDescriptor, VertexDescriptor,
};
use metal2vulkan_runtime::{
    ArgumentEncoder, BlitCommandEncoder, BlitOption, Buffer, ClearColor, CommandBuffer,
    CommandBufferStatus, CommandQueue, ComputeCommandEncoder, ComputePipelineState, Deallocator,
    DepthStencilState, Device, DeviceOptions, Event, EventRef, Fence, Function,
    FunctionDescriptor, HeapDescriptor, HeapType, HostMemory, IoSurfacePlane, IoSurfaceYCbCr,
    Library, LoadAction, Origin,
    PipelineOptions, Region, RenderCommandEncoder, RenderPassDescriptor, RenderPipelineState,
    ResourceOptions, ResourceRef, SamplerState, ScissorRect, ShaderCacheOptions, SharedEvent,
    SharedEventHandle, SharedEventListener, Size, StoreAction, SurfaceYCbCrMatrix, Texture,
    TextureDescriptor, Viewport,
};
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::time::Duration;

thread_local! {
    static LAST_ERR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn set_error(msg: impl Into<String>) {
    let s = msg.into();
    LAST_ERR.with(|cell| {
        let mut buf = cell.borrow_mut();
        buf.clear();
        buf.extend(s.bytes().filter(|&b| b != 0));
        buf.push(0);
    });
}

fn guard<T>(fallback: T, f: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            set_error(e);
            fallback
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| p.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            set_error(format!("panic: {msg}"));
            fallback
        }
    }
}

fn boxed<T>(v: T) -> *mut T {
    Box::into_raw(Box::new(v))
}

fn get<'a, T>(p: *const T) -> Result<&'a T, String> {
    unsafe { p.as_ref() }.ok_or_else(|| "null handle".to_string())
}

fn get_mut<'a, T>(p: *mut T) -> Result<&'a mut T, String> {
    unsafe { p.as_mut() }.ok_or_else(|| "null handle".to_string())
}

fn take_box<T>(p: *mut T) -> Result<Box<T>, String> {
    if p.is_null() {
        Err("null handle".to_string())
    } else {
        Ok(unsafe { Box::from_raw(p) })
    }
}

fn release<T>(p: *mut T) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p) });
    }
}

fn copy_str(s: &str, out: *mut c_char, size: usize) -> usize {
    if !out.is_null() && size > 0 {
        let n = s.len().min(size - 1);
        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), out.cast::<u8>(), n);
            *out.add(n) = 0;
        }
    }
    s.len()
}

fn cstr_opt(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn raw_enum<T: TryFrom<u32>>(v: u64, what: &str) -> Result<T, String>
where
    T::Error: std::fmt::Debug,
{
    u32::try_from(v)
        .ok()
        .and_then(|x| T::try_from(x).ok())
        .ok_or_else(|| format!("invalid {what}: {v}"))
}

fn raw_opts(v: u64) -> Result<ResourceOptions, String> {
    ResourceOptions::from_raw(v).map_err(|e| format!("resource options {v:#x}: {e:?}"))
}

fn raw_load(v: u64) -> Result<LoadAction, String> {
    match v {
        0 => Ok(LoadAction::DontCare),
        1 => Ok(LoadAction::Load),
        2 => Ok(LoadAction::Clear),
        _ => Err(format!("invalid load_action: {v}")),
    }
}

fn raw_store(v: u64) -> Result<StoreAction, String> {
    match v {
        0 => Ok(StoreAction::DontCare),
        1 => Ok(StoreAction::Store),
        2 => Ok(StoreAction::MultisampleResolve),
        3 => Ok(StoreAction::StoreAndMultisampleResolve),
        _ => Err(format!("invalid store_action: {v}")),
    }
}

/* ---- C descriptor structs ---- */

#[repr(C)]
pub struct Xm2vTextureDesc {
    pub texture_type: u64,
    pub pixel_format: u64,
    pub width: u64,
    pub height: u64,
    pub depth: u64,
    pub mipmap_level_count: u64,
    pub sample_count: u64,
    pub array_length: u64,
    pub resource_options: u64,
    pub usage: u64,
}

#[repr(C)]
pub struct Xm2vColorAttachmentDesc {
    pub pixel_format: u64,
    pub blending_enabled: bool,
    pub rgb_blend_op: u64,
    pub alpha_blend_op: u64,
    pub src_rgb_blend: u64,
    pub dst_rgb_blend: u64,
    pub src_alpha_blend: u64,
    pub dst_alpha_blend: u64,
    pub write_mask: u64,
}

#[repr(C)]
pub struct Xm2vVertexAttribute {
    pub index: u32,
    pub format: u64,
    pub offset: u32,
    pub buffer_index: u32,
}

#[repr(C)]
pub struct Xm2vVertexLayout {
    pub index: u32,
    pub stride: u64,
    pub step_function: u64,
    pub step_rate: u32,
    pub dynamic_stride: bool,
}

#[repr(C)]
pub struct Xm2vRenderPipelineDesc {
    pub vertex_function: *mut Function,
    pub fragment_function: *mut Function,
    pub color_attachment_count: u32,
    pub color_attachments: [Xm2vColorAttachmentDesc; 8],
    pub depth_attachment_pixel_format: u64,
    pub stencil_attachment_pixel_format: u64,
    pub sample_count: u64,
    pub vertex_linked_functions: *const *mut Function,
    pub vertex_linked_function_count: u32,
    pub vertex_private_linked_functions: *const *mut Function,
    pub vertex_private_linked_function_count: u32,
    pub fragment_linked_functions: *const *mut Function,
    pub fragment_linked_function_count: u32,
    pub fragment_private_linked_functions: *const *mut Function,
    pub fragment_private_linked_function_count: u32,
    pub vertex_attributes: *const Xm2vVertexAttribute,
    pub vertex_attribute_count: u32,
    pub vertex_layouts: *const Xm2vVertexLayout,
    pub vertex_layout_count: u32,
}

#[repr(C)]
pub struct Xm2vRenderPassColorAttachment {
    pub texture: *mut Texture,
    pub level: u64,
    pub slice: u64,
    pub load_action: u64,
    pub store_action: u64,
    pub clear_r: f64,
    pub clear_g: f64,
    pub clear_b: f64,
    pub clear_a: f64,
}

#[repr(C)]
pub struct Xm2vRenderPassDesc {
    pub color_attachment_count: u32,
    pub color_attachments: [Xm2vRenderPassColorAttachment; 8],
    pub depth_texture: *mut Texture,
    pub depth_load_action: u64,
    pub depth_store_action: u64,
    pub clear_depth: f64,
    pub stencil_texture: *mut Texture,
    pub stencil_load_action: u64,
    pub stencil_store_action: u64,
    pub clear_stencil: u32,
}

#[repr(C)]
pub struct Xm2vSamplerDesc {
    pub min_filter: u64,
    pub mag_filter: u64,
    pub mip_filter: u64,
    pub s_address_mode: u64,
    pub t_address_mode: u64,
    pub r_address_mode: u64,
    pub max_anisotropy: u64,
    pub compare_function: u64,
    pub lod_min_clamp: f32,
    pub lod_max_clamp: f32,
    pub normalized_coordinates: bool,
}

#[repr(C)]
pub struct Xm2vStencilDesc {
    pub enabled: bool,
    pub stencil_compare_function: u64,
    pub stencil_failure_operation: u64,
    pub depth_failure_operation: u64,
    pub depth_stencil_pass_operation: u64,
    pub read_mask: u32,
    pub write_mask: u32,
}

#[repr(C)]
pub struct Xm2vDepthStencilDesc {
    pub depth_compare_function: u64,
    pub depth_write_enabled: bool,
    pub front_face_stencil: Xm2vStencilDesc,
    pub back_face_stencil: Xm2vStencilDesc,
}

/* ---- Error ---- */

#[no_mangle]
pub extern "C" fn xm2v_last_error() -> *const c_char {
    LAST_ERR.with(|cell| {
        let buf = cell.borrow();
        if buf.is_empty() {
            c"".as_ptr()
        } else {
            buf.as_ptr().cast::<c_char>()
        }
    })
}

/* ---- Device ---- */

#[no_mangle]
pub extern "C" fn xm2v_device_open(
    loader_path: *const c_char,
    icd_json_path: *const c_char,
    cache_dir: *const c_char,
) -> *mut Device {
    xm2v_device_open_direct(loader_path, std::ptr::null_mut(), icd_json_path, cache_dir)
}

#[no_mangle]
pub extern "C" fn xm2v_device_open_direct(
    loader_path: *const c_char,
    direct_driver_pfn: *mut c_void,
    icd_json_path: *const c_char,
    cache_dir: *const c_char,
) -> *mut Device {
    guard(std::ptr::null_mut(), || {
        let use_direct = !direct_driver_pfn.is_null();
        let prev_icd = std::env::var_os("VK_ICD_FILENAMES");
        let prev_drv = std::env::var_os("VK_DRIVER_FILES");
        let icd_opt = if use_direct {
            None
        } else {
            cstr_opt(icd_json_path)
        };
        if let Some(ref icd) = icd_opt {
            unsafe {
                std::env::set_var("VK_ICD_FILENAMES", icd);
                std::env::set_var("VK_DRIVER_FILES", icd);
            }
        }
        let mut opts = DeviceOptions::default();
        if let Some(loader) = cstr_opt(loader_path) {
            opts.loader = Some(PathBuf::from(loader));
        }
        if use_direct {
            opts.direct_driver_pfn = Some(direct_driver_pfn as usize);
        }
        if let Some(cache) = cstr_opt(cache_dir) {
            let mut sc = ShaderCacheOptions::in_directory(cache);
            let disk_mb: u64 = std::env::var("XNU_CPU_METAL_CACHE_MAX_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&v| v >= 16)
                .unwrap_or(512);
            let mem_mb: u64 = std::env::var("XNU_CPU_METAL_MEM_CACHE_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&v| v >= 4)
                .unwrap_or(32);
            sc.capacity = disk_mb << 20;
            sc.memory_capacity = mem_mb << 20;
            opts.shader_cache = sc;
        }
        let res = Device::system_default_with(&opts).map_err(|e| format!("device open: {e:?}"));
        if icd_opt.is_some() {
            unsafe {
                match prev_icd {
                    Some(v) => std::env::set_var("VK_ICD_FILENAMES", v),
                    None => std::env::remove_var("VK_ICD_FILENAMES"),
                }
                match prev_drv {
                    Some(v) => std::env::set_var("VK_DRIVER_FILES", v),
                    None => std::env::remove_var("VK_DRIVER_FILES"),
                }
            }
        }
        Ok(boxed(res?))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_device_release(dev: *mut Device) {
    release(dev);
}

#[no_mangle]
pub extern "C" fn xm2v_device_name(dev: *const Device, out: *mut c_char, size: usize) -> usize {
    guard(0, || {
        let dev = get(dev)?;
        Ok(copy_str(&dev.name(), out, size))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_device_max_buffer_length(dev: *const Device) -> u64 {
    guard(0, || Ok(get(dev)?.properties().max_buffer_length))
}

#[no_mangle]
pub extern "C" fn xm2v_device_max_threads_per_threadgroup(
    dev: *const Device,
    out: *mut u64,
) -> bool {
    guard(false, || {
        let dev = get(dev)?;
        if out.is_null() {
            return Err("null out".to_string());
        }
        let s = dev.properties().max_threads_per_threadgroup;
        unsafe {
            *out.add(0) = s.width;
            *out.add(1) = s.height;
            *out.add(2) = s.depth;
        }
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_device_supports_family(dev: *const Device, family: i64) -> bool {
    guard(false, || Ok(get(dev)?.supports_family(family)))
}

/* ---- Buffers ---- */

#[no_mangle]
pub extern "C" fn xm2v_buffer_new(
    dev: *mut Device,
    length: u64,
    resource_options: u64,
    bytes: *const c_void,
) -> *mut Buffer {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let opts = raw_opts(resource_options)?;
        let buf = if bytes.is_null() {
            dev.new_buffer(length, opts)
        } else {
            let slice = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), length as usize) };
            dev.new_buffer_with_bytes(slice, opts)
        }
        .map_err(|e| format!("new_buffer: {e:?}"))?;
        Ok(boxed(buf))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_new_page_aligned(
    dev: *mut Device,
    length: u64,
    resource_options: u64,
    bytes: *const c_void,
) -> *mut Buffer {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let opts = raw_opts(resource_options)?;
        // A dedicated Placement heap allocates its own VkDeviceMemory at offset 0,
        // guaranteeing a page-aligned host mapping for mach_vm_remap physical-page aliasing.
        let mut hd = HeapDescriptor::default();
        hd.heap_type = HeapType::Placement;
        hd.size = length;
        hd.resource_options = opts;
        let buf = dev
            .new_heap(&hd)
            .and_then(|h| h.new_buffer_at_offset(length, opts, 0))
            .or_else(|_| dev.new_buffer(length, opts))
            .map_err(|e| format!("new_buffer_page_aligned: {e:?}"))?;
        if !bytes.is_null() && length > 0 {
            if let Some(dst) = buf.contents() {
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.cast::<u8>(), dst.as_ptr(), length as usize);
                }
                if (resource_options & 0xF0) == 0x10 {
                    let _ = buf.did_modify_range(0..length);
                }
            }
        }
        Ok(boxed(buf))
    })
}

pub type Xm2vDeallocatorFn = extern "C" fn(*mut c_void, u64, *mut c_void);
pub type Xm2vSharedEventNotifyFn = extern "C" fn(*mut c_void, u64);

fn build_texture_descriptor(d: &Xm2vTextureDesc) -> Result<TextureDescriptor, String> {
    let pixel_format: PixelFormat = raw_enum(d.pixel_format, "PixelFormat")?;
    let texture_type: TextureType = raw_enum(d.texture_type, "TextureType")?;
    let mut td = TextureDescriptor::texture_2d(
        pixel_format,
        d.width.max(1),
        d.height.max(1),
        d.mipmap_level_count > 1,
    );
    td.texture_type = texture_type;
    td.depth = d.depth.max(1);
    td.mipmap_level_count = d.mipmap_level_count.max(1);
    td.sample_count = d.sample_count.max(1);
    td.array_length = d.array_length.max(1);
    td.resource_options = raw_opts(d.resource_options)?;
    td.usage = raw_enum::<TextureUsage>(d.usage, "TextureUsage")?;
    Ok(td)
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_new_no_copy(
    dev: *mut Device,
    ptr: *mut c_void,
    length: u64,
    resource_options: u64,
) -> *mut Buffer {
    xm2v_buffer_new_no_copy_dealloc(dev, ptr, length, resource_options, None, std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_new_no_copy_dealloc(
    dev: *mut Device,
    ptr: *mut c_void,
    length: u64,
    resource_options: u64,
    dealloc_fn: Option<Xm2vDeallocatorFn>,
    dealloc_ctx: *mut c_void,
) -> *mut Buffer {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let opts = raw_opts(resource_options)?;
        let nn = NonNull::new(ptr.cast::<u8>()).ok_or_else(|| "null ptr".to_string())?;
        let hm = unsafe { HostMemory::new(nn) };
        let dealloc: Option<Deallocator> = dealloc_fn.map(|f| {
            let ctx_addr = dealloc_ctx as usize;
            Box::new(move |p: NonNull<u8>, len: u64| {
                f(p.as_ptr().cast::<c_void>(), len, ctx_addr as *mut c_void);
            }) as Deallocator
        });
        let buf = unsafe { dev.new_buffer_with_bytes_no_copy(hm, length, opts, dealloc) }
            .map_err(|e| format!("new_buffer_no_copy: {e:?}"))?;
        Ok(boxed(buf))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_new_texture(
    buf: *mut Buffer,
    desc: *const Xm2vTextureDesc,
    offset: u64,
    bytes_per_row: u64,
) -> *mut Texture {
    guard(std::ptr::null_mut(), || {
        let buf = get(buf)?;
        let td = build_texture_descriptor(get(desc)?)?;
        let tex = buf
            .new_texture(&td, offset, bytes_per_row)
            .map_err(|e| format!("buffer.new_texture(off={offset}, bpr={bytes_per_row}): {e:?}"))?;
        Ok(boxed(tex))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_new_linear_texture(
    buf: *mut Buffer,
    desc: *const Xm2vTextureDesc,
    offset: u64,
    bytes_per_row: u64,
    bytes_per_image: u64,
) -> *mut Texture {
    guard(std::ptr::null_mut(), || {
        let buf = get(buf)?;
        let td = build_texture_descriptor(get(desc)?)?;
        let tex = buf
            .new_linear_texture(&td, offset, bytes_per_row, bytes_per_image)
            .map_err(|e| {
                format!(
                    "buffer.new_linear_texture(off={offset}, bpr={bytes_per_row}, bpi={bytes_per_image}): {e:?}"
                )
            })?;
        Ok(boxed(tex))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_pixel_format_is_ycbcr(pixel_format: u64) -> bool {
    guard(false, || {
        let pf: PixelFormat = raw_enum(pixel_format, "PixelFormat")?;
        Ok(metal2vulkan::metal::format::ycbcr(pf))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_new_texture_with_iosurface_ycbcr(
    buf: *mut Buffer,
    desc: *const Xm2vTextureDesc,
    luma_offset: u64,
    luma_bytes_per_row: u64,
    planar: bool,
    surface_pixel_format: u32,
    surface_plane: u64,
    surface_width: u64,
    surface_height: u64,
    chroma_offset: u64,
    chroma_bytes_per_row: u64,
    ycbcr_matrix: *const c_char,
) -> *mut Texture {
    guard(std::ptr::null_mut(), || {
        let buf = get(buf)?;
        let td = build_texture_descriptor(get(desc)?)?;
        let mut facts = IoSurfaceYCbCr::new(
            surface_pixel_format,
            surface_plane,
            [surface_width, surface_height],
            chroma_offset,
            chroma_bytes_per_row,
        );
        if !ycbcr_matrix.is_null() {
            if let Ok(s) = unsafe { CStr::from_ptr(ycbcr_matrix) }.to_str() {
                if !s.is_empty() {
                    facts = facts.with_matrix(SurfaceYCbCrMatrix::from_attachment(s));
                }
            }
        }
        let plane = IoSurfacePlane::new(luma_offset, luma_bytes_per_row, planar).with_ycbcr(facts);
        let tex = buf
            .new_texture_with_iosurface_plane(&td, plane)
            .map_err(|e| {
                format!(
                    "buffer.new_texture_with_iosurface_plane(fmt={:?} ostype=0x{surface_pixel_format:08x} plane={surface_plane} luma=({luma_offset},{luma_bytes_per_row}) chroma=({chroma_offset},{chroma_bytes_per_row})): {e:?}",
                    td.pixel_format
                )
            })?;
        Ok(boxed(tex))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_release(buf: *mut Buffer) {
    release(buf);
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_contents(buf: *const Buffer) -> *mut c_void {
    guard(std::ptr::null_mut(), || {
        Ok(get(buf)?
            .contents()
            .map(|p| p.as_ptr().cast::<c_void>())
            .unwrap_or(std::ptr::null_mut()))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_length(buf: *const Buffer) -> u64 {
    guard(0, || Ok(get(buf)?.length()))
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_gpu_address(buf: *const Buffer) -> u64 {
    guard(0, || Ok(get(buf)?.gpu_address().0))
}

#[no_mangle]
pub extern "C" fn xm2v_buffer_did_modify_range(buf: *const Buffer, location: u64, length: u64) {
    let _ = guard((), || {
        let _ = get(buf)?.did_modify_range(location..location.saturating_add(length));
        Ok(())
    });
}

/* ---- Textures ---- */

#[no_mangle]
pub extern "C" fn xm2v_texture_new(
    dev: *mut Device,
    desc: *const Xm2vTextureDesc,
) -> *mut Texture {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let td = build_texture_descriptor(get(desc)?)?;
        let tex = dev
            .new_texture(&td)
            .map_err(|e| format!("new_texture: {e:?}"))?;
        Ok(boxed(tex))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_texture_new_view(
    tex: *mut Texture,
    pixel_format: u64,
    texture_type: u64,
    level_start: u64,
    level_len: u64,
    slice_start: u64,
    slice_len: u64,
) -> *mut Texture {
    guard(std::ptr::null_mut(), || {
        let tex = get(tex)?;
        let pf: PixelFormat = raw_enum(pixel_format, "PixelFormat")?;
        let tt: TextureType = raw_enum(texture_type, "TextureType")?;
        let view = tex
            .new_texture_view_with_levels_slices(
                pf,
                tt,
                level_start..level_start.saturating_add(level_len),
                slice_start..slice_start.saturating_add(slice_len),
            )
            .map_err(|e| format!("new_texture_view: {e:?}"))?;
        Ok(boxed(view))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_texture_release(tex: *mut Texture) {
    release(tex);
}

#[no_mangle]
pub extern "C" fn xm2v_texture_gpu_resource_id(tex: *const Texture) -> u64 {
    guard(0, || Ok(get(tex)?.gpu_resource_id().0))
}

#[no_mangle]
pub extern "C" fn xm2v_texture_replace_region(
    tex: *const Texture,
    x: u64,
    y: u64,
    z: u64,
    w: u64,
    h: u64,
    d: u64,
    mip_level: u64,
    slice: u64,
    bytes: *const c_void,
    bytes_per_row: u64,
    bytes_per_image: u64,
) -> bool {
    guard(false, || {
        let tex = get(tex)?;
        if bytes.is_null() {
            return Err("null bytes".to_string());
        }
        let total = if bytes_per_image > 0 {
            bytes_per_image * d.max(1)
        } else {
            bytes_per_row * h.max(1) * d.max(1)
        };
        let src = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), total as usize) };
        let region = Region::new_3d(x, y, z, w, h, d);
        tex.replace_region(region, mip_level, slice, src, bytes_per_row, bytes_per_image)
            .map_err(|e| format!("replace_region: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_texture_get_bytes(
    tex: *const Texture,
    out_bytes: *mut c_void,
    bytes_per_row: u64,
    bytes_per_image: u64,
    x: u64,
    y: u64,
    z: u64,
    w: u64,
    h: u64,
    d: u64,
    mip_level: u64,
    slice: u64,
) -> bool {
    guard(false, || {
        let tex = get(tex)?;
        if out_bytes.is_null() {
            return Err("null out_bytes".to_string());
        }
        let total = if bytes_per_image > 0 {
            bytes_per_image * d.max(1)
        } else {
            bytes_per_row * h.max(1) * d.max(1)
        };
        let dst = unsafe { std::slice::from_raw_parts_mut(out_bytes.cast::<u8>(), total as usize) };
        let region = Region::new_3d(x, y, z, w, h, d);
        tex.get_bytes(dst, bytes_per_row, bytes_per_image, region, mip_level, slice)
            .map_err(|e| format!("get_bytes: {e:?}"))?;
        Ok(true)
    })
}

/* ---- Libraries & Functions ---- */

#[no_mangle]
pub extern "C" fn xm2v_library_new_with_data(
    dev: *mut Device,
    bytes: *const c_void,
    len: usize,
) -> *mut Library {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        if bytes.is_null() || len == 0 {
            return Err("empty library data".to_string());
        }
        let slice = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) };
        let lib = dev
            .new_library_with_data(slice)
            .map_err(|e| format!("new_library_with_data: {e:?}"))?;
        Ok(boxed(lib))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_library_release(lib: *mut Library) {
    release(lib);
}

#[no_mangle]
pub extern "C" fn xm2v_library_function_count(lib: *const Library) -> usize {
    guard(0, || Ok(get(lib)?.function_names().len()))
}

#[no_mangle]
pub extern "C" fn xm2v_library_function_name_at(
    lib: *const Library,
    index: usize,
    out: *mut c_char,
    size: usize,
) -> usize {
    guard(0, || {
        let names = get(lib)?.function_names();
        let name = names
            .get(index)
            .ok_or_else(|| format!("function index {index} out of bounds"))?;
        Ok(copy_str(name, out, size))
    })
}

#[repr(C)]
pub struct Xm2vConstant {
    pub data_type: u64,
    pub index: u32,
    pub name: *const c_char,
    pub bytes: *const c_void,
    pub length: usize,
}

#[no_mangle]
pub extern "C" fn xm2v_library_extern_function_count(lib: *const Library) -> usize {
    guard(0, || Ok(get(lib)?.extern_function_names().len()))
}

#[no_mangle]
pub extern "C" fn xm2v_library_extern_function_name_at(
    lib: *const Library,
    index: usize,
    out: *mut c_char,
    size: usize,
) -> usize {
    guard(0, || {
        let names = get(lib)?.extern_function_names();
        let name = names
            .get(index)
            .ok_or_else(|| format!("extern function index {index} out of bounds"))?;
        Ok(copy_str(name, out, size))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_library_new_function(
    lib: *mut Library,
    name: *const c_char,
) -> *mut Function {
    guard(std::ptr::null_mut(), || {
        let lib = get(lib)?;
        let name = cstr_opt(name).ok_or_else(|| "empty function name".to_string())?;
        let f = lib
            .new_function_with_name(&name)
            .map_err(|e| format!("new_function({name}): {e:?}"))?;
        Ok(boxed(f))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_library_new_extern_function(
    lib: *mut Library,
    name: *const c_char,
) -> *mut Function {
    guard(std::ptr::null_mut(), || {
        let lib = get(lib)?;
        let name = cstr_opt(name).ok_or_else(|| "empty extern function name".to_string())?;
        let f = lib
            .new_extern_function_with_name(&name)
            .map_err(|e| format!("new_extern_function({name}): {e:?}"))?;
        Ok(boxed(f))
    })
}

fn build_fcv(constants: *const Xm2vConstant, count: u32) -> Result<FunctionConstantValues, String> {
    let mut fcv = FunctionConstantValues::defaults();
    if count > 0 && !constants.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(constants, count as usize) };
        for c in slice {
            let ty: DataType = raw_enum(c.data_type, "DataType")?;
            let bytes = if c.bytes.is_null() || c.length == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(c.bytes.cast::<u8>(), c.length) }.to_vec()
            };
            let val = FunctionConstantValue::new(ty, bytes);
            if let Some(cname) = cstr_opt(c.name) {
                fcv = fcv.with_name(cname, val);
            } else {
                fcv = fcv.with_index(c.index, val);
            }
        }
    }
    Ok(fcv)
}

fn build_linked(
    public_fns: *const *mut Function,
    public_count: u32,
    private_fns: *const *mut Function,
    private_count: u32,
) -> Result<LinkedFunctions, String> {
    let mut linked = LinkedFunctions::new();
    if public_count > 0 && !public_fns.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(public_fns, public_count as usize) };
        for &fp in slice {
            if !fp.is_null() {
                linked = linked.with_function(get(fp)?.linked_function());
            }
        }
    }
    if private_count > 0 && !private_fns.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(private_fns, private_count as usize) };
        for &fp in slice {
            if !fp.is_null() {
                linked = linked.with_private_function(get(fp)?.linked_function());
            }
        }
    }
    Ok(linked)
}

fn constant_bytes(raw: u32) -> usize {
    match raw {
        3..=6 => (raw as usize - 2) * 4,
        16..=19 => (raw as usize - 15) * 2,
        29..=32 => (raw as usize - 28) * 4,
        33..=36 => (raw as usize - 32) * 4,
        37..=40 => (raw as usize - 36) * 2,
        41..=44 => (raw as usize - 40) * 2,
        45..=48 => raw as usize - 44,
        49..=52 => raw as usize - 48,
        53..=56 => raw as usize - 52,
        81..=84 => (raw as usize - 80) * 8,
        85..=88 => (raw as usize - 84) * 8,
        121..=124 => (raw as usize - 120) * 2,
        _ => 0,
    }
}

fn fill_zero_rest(lib: &Library, name: &str, fcv: &mut FunctionConstantValues) -> Result<(), String> {
    let generic = lib
        .new_function_with_name(name)
        .map_err(|e| format!("new_function({name}): {e:?}"))?;
    let dict = generic
        .function_constants_dictionary()
        .map_err(|e| format!("function_constants_dictionary({name}): {e:?}"))?;
    for (key, constant) in dict {
        if fcv.by_name.contains_key(&key) || fcv.by_index.contains_key(&constant.index()) {
            continue;
        }
        let ty = constant.data_type();
        let sz = constant_bytes(ty.as_raw());
        if sz > 0 {
            fcv.by_index
                .insert(constant.index(), FunctionConstantValue::new(ty, vec![0u8; sz]));
        }
    }
    Ok(())
}

#[no_mangle]
pub extern "C" fn xm2v_library_new_function_specialized(
    lib: *mut Library,
    name: *const c_char,
    constants: *const Xm2vConstant,
    count: u32,
) -> *mut Function {
    guard(std::ptr::null_mut(), || {
        let lib = get(lib)?;
        let name = cstr_opt(name).ok_or_else(|| "empty function name".to_string())?;
        let fcv = build_fcv(constants, count)?;
        let f = lib
            .new_function_with_name_constant_values(&name, &fcv)
            .map_err(|e| format!("new_function_specialized({name}): {e:?}"))?;
        Ok(boxed(f))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_library_new_function_specialized_zero_rest(
    lib: *mut Library,
    name: *const c_char,
    constants: *const Xm2vConstant,
    count: u32,
) -> *mut Function {
    guard(std::ptr::null_mut(), || {
        let lib = get(lib)?;
        let name = cstr_opt(name).ok_or_else(|| "empty function name".to_string())?;
        let mut fcv = build_fcv(constants, count)?;
        fill_zero_rest(lib, &name, &mut fcv)?;
        let f = lib
            .new_function_with_name_constant_values(&name, &fcv)
            .map_err(|e| format!("new_function_specialized_zero_rest({name}): {e:?}"))?;
        Ok(boxed(f))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_library_new_function_with_descriptor(
    lib: *mut Library,
    name: *const c_char,
    specialized_name: *const c_char,
    constants: *const Xm2vConstant,
    count: u32,
    options: u64,
) -> *mut Function {
    guard(std::ptr::null_mut(), || {
        let lib = get(lib)?;
        let name = cstr_opt(name).ok_or_else(|| "empty function name".to_string())?;
        let mut fd = FunctionDescriptor::new(&name);
        if let Some(sname) = cstr_opt(specialized_name) {
            fd = fd.with_specialized_name(sname);
        }
        if count > 0 && !constants.is_null() {
            fd.constant_values = Some(build_fcv(constants, count)?);
        }
        fd.options = raw_enum::<FunctionOptions>(options, "FunctionOptions")?;
        let f = lib
            .new_function_with_descriptor(&fd)
            .map_err(|e| format!("new_function_with_descriptor({name}): {e:?}"))?;
        Ok(boxed(f))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_function_release(f: *mut Function) {
    release(f);
}

#[no_mangle]
pub extern "C" fn xm2v_function_type(f: *const Function) -> u32 {
    guard(0, || {
        let ft = get(f)?
            .function_type()
            .map_err(|e| format!("function_type: {e:?}"))?;
        Ok(ft as u32)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_function_new_argument_encoder(
    f: *mut Function,
    buffer_index: u32,
) -> *mut ArgumentEncoder {
    guard(std::ptr::null_mut(), || {
        let enc = get(f)?
            .new_argument_encoder_with_buffer_index(buffer_index)
            .map_err(|e| format!("new_argument_encoder({buffer_index}): {e:?}"))?;
        Ok(boxed(enc))
    })
}

/* ---- Argument Encoder ---- */

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_release(enc: *mut ArgumentEncoder) {
    release(enc);
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_encoded_length(enc: *const ArgumentEncoder) -> u64 {
    guard(0, || Ok(get(enc)?.encoded_length()))
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_alignment(enc: *const ArgumentEncoder) -> u64 {
    guard(1, || Ok(get(enc)?.alignment()))
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_set_argument_buffer(
    enc: *mut ArgumentEncoder,
    buf: *mut Buffer,
    offset: u64,
) -> bool {
    guard(false, || {
        let b = if buf.is_null() { None } else { Some(get(buf)?) };
        get_mut(enc)?
            .set_argument_buffer(b, offset)
            .map_err(|e| format!("set_argument_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_set_argument_buffer_array_element(
    enc: *mut ArgumentEncoder,
    buf: *mut Buffer,
    start_offset: u64,
    array_element: u64,
) -> bool {
    guard(false, || {
        let b = if buf.is_null() { None } else { Some(get(buf)?) };
        get_mut(enc)?
            .set_argument_buffer_start_offset_array_element(b, start_offset, array_element)
            .map_err(|e| format!("set_argument_buffer_array_element: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_set_buffer(
    enc: *const ArgumentEncoder,
    buf: *mut Buffer,
    offset: u64,
    index: u32,
) -> bool {
    guard(false, || {
        let b = if buf.is_null() { None } else { Some(get(buf)?) };
        get(enc)?
            .set_buffer(b, offset, index)
            .map_err(|e| format!("arg_enc set_buffer({index}): {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_set_texture(
    enc: *const ArgumentEncoder,
    tex: *mut Texture,
    index: u32,
) -> bool {
    guard(false, || {
        let t = if tex.is_null() { None } else { Some(get(tex)?) };
        get(enc)?
            .set_texture(t, index)
            .map_err(|e| format!("arg_enc set_texture({index}): {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_set_sampler(
    enc: *const ArgumentEncoder,
    samp: *mut SamplerState,
    index: u32,
) -> bool {
    guard(false, || {
        let s = if samp.is_null() { None } else { Some(get(samp)?) };
        get(enc)?
            .set_sampler_state(s, index)
            .map_err(|e| format!("arg_enc set_sampler({index}): {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_set_compute_pipeline(
    enc: *const ArgumentEncoder,
    pso: *mut ComputePipelineState,
    index: u32,
) -> bool {
    guard(false, || {
        let p = if pso.is_null() { None } else { Some(get(pso)?) };
        get(enc)?
            .set_compute_pipeline_state(p, index)
            .map_err(|e| format!("arg_enc set_compute_pipeline({index}): {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_set_render_pipeline(
    enc: *const ArgumentEncoder,
    pso: *mut RenderPipelineState,
    index: u32,
) -> bool {
    guard(false, || {
        let p = if pso.is_null() { None } else { Some(get(pso)?) };
        get(enc)?
            .set_render_pipeline_state(p, index)
            .map_err(|e| format!("arg_enc set_render_pipeline({index}): {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_constant_data(
    enc: *const ArgumentEncoder,
    index: u32,
) -> *mut c_void {
    guard(std::ptr::null_mut(), || {
        let ptr = get(enc)?
            .constant_data(index)
            .map_err(|e| format!("arg_enc constant_data({index}): {e:?}"))?;
        Ok(ptr.as_ptr().cast::<c_void>())
    })
}

#[no_mangle]
pub extern "C" fn xm2v_argument_encoder_new_nested(
    enc: *const ArgumentEncoder,
    index: u32,
) -> *mut ArgumentEncoder {
    guard(std::ptr::null_mut(), || {
        let nested = get(enc)?
            .new_argument_encoder_for_buffer_at_index(index)
            .map_err(|e| format!("arg_enc new_nested({index}): {e:?}"))?;
        Ok(boxed(nested))
    })
}

/* ---- Pipelines & States ---- */

#[no_mangle]
pub extern "C" fn xm2v_compute_pipeline_new(
    dev: *mut Device,
    f: *mut Function,
) -> *mut ComputePipelineState {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let f = get(f)?;
        let pso = dev
            .new_compute_pipeline_state_with_function(f, &PipelineOptions::default())
            .map_err(|e| format!("compute pipeline: {e:?}"))?;
        Ok(boxed(pso))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_pipeline_new_with_linked(
    dev: *mut Device,
    f: *mut Function,
    linked_fns: *const *mut Function,
    linked_count: u32,
    private_fns: *const *mut Function,
    private_count: u32,
) -> *mut ComputePipelineState {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let f = get(f)?;
        let linked = build_linked(linked_fns, linked_count, private_fns, private_count)?;
        let fd = if linked.is_empty() {
            f.function_descriptor()
        } else {
            f.function_descriptor().with_linked(linked)
        };
        let cpd = ComputePipelineDescriptor::new(StageSource::Function(fd));
        let pso = dev
            .new_compute_pipeline_state(&cpd, &PipelineOptions::default())
            .map_err(|e| format!("compute pipeline: {e:?}"))?;
        Ok(boxed(pso))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_pipeline_release(pso: *mut ComputePipelineState) {
    release(pso);
}

#[no_mangle]
pub extern "C" fn xm2v_compute_pipeline_max_threads(pso: *const ComputePipelineState) -> u64 {
    guard(0, || Ok(get(pso)?.max_total_threads_per_threadgroup() as u64))
}

#[no_mangle]
pub extern "C" fn xm2v_compute_pipeline_thread_width(pso: *const ComputePipelineState) -> u64 {
    guard(0, || Ok(get(pso)?.thread_execution_width() as u64))
}

#[no_mangle]
pub extern "C" fn xm2v_compute_pipeline_static_tg_mem(pso: *const ComputePipelineState) -> u64 {
    guard(0, || Ok(get(pso)?.static_threadgroup_memory_length() as u64))
}

#[no_mangle]
pub extern "C" fn xm2v_render_pipeline_new(
    dev: *mut Device,
    desc: *const Xm2vRenderPipelineDesc,
) -> *mut RenderPipelineState {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let d = get(desc)?;
        let vfn = get(d.vertex_function)?;
        let vlinked = build_linked(
            d.vertex_linked_functions,
            d.vertex_linked_function_count,
            d.vertex_private_linked_functions,
            d.vertex_private_linked_function_count,
        )?;
        let vfd = if vlinked.is_empty() {
            vfn.function_descriptor()
        } else {
            vfn.function_descriptor().with_linked(vlinked)
        };
        let mut rpd = RenderPipelineDescriptor::new(StageSource::Function(vfd));
        if !d.fragment_function.is_null() {
            let ffn = get(d.fragment_function)?;
            let flinked = build_linked(
                d.fragment_linked_functions,
                d.fragment_linked_function_count,
                d.fragment_private_linked_functions,
                d.fragment_private_linked_function_count,
            )?;
            let ffd = if flinked.is_empty() {
                ffn.function_descriptor()
            } else {
                ffn.function_descriptor().with_linked(flinked)
            };
            rpd.fragment = Some(StageSource::Function(ffd));
        }
        let count = (d.color_attachment_count as usize).min(8);
        for i in 0..count {
            let ca = &d.color_attachments[i];
            if ca.pixel_format == 0 {
                continue;
            }
            let pf: PixelFormat = raw_enum(ca.pixel_format, "PixelFormat")?;
            let mut cad = ColorAttachmentDescriptor::new(pf);
            cad.blending_enabled = ca.blending_enabled;
            if ca.blending_enabled {
                cad.rgb_blend_operation = raw_enum::<BlendOperation>(ca.rgb_blend_op, "BlendOp")?;
                cad.alpha_blend_operation = raw_enum::<BlendOperation>(ca.alpha_blend_op, "BlendOp")?;
                cad.source_rgb_blend_factor = raw_enum::<BlendFactor>(ca.src_rgb_blend, "BlendFactor")?;
                cad.destination_rgb_blend_factor = raw_enum::<BlendFactor>(ca.dst_rgb_blend, "BlendFactor")?;
                cad.source_alpha_blend_factor = raw_enum::<BlendFactor>(ca.src_alpha_blend, "BlendFactor")?;
                cad.destination_alpha_blend_factor = raw_enum::<BlendFactor>(ca.dst_alpha_blend, "BlendFactor")?;
            }
            cad.write_mask = raw_enum::<ColorWriteMask>(ca.write_mask, "ColorWriteMask")?;
            rpd.color_attachments[i] = Some(cad);
        }
        if d.depth_attachment_pixel_format != 0 {
            rpd.depth_format = Some(raw_enum(d.depth_attachment_pixel_format, "DepthFormat")?);
        }
        if d.stencil_attachment_pixel_format != 0 {
            rpd.stencil_format = Some(raw_enum(d.stencil_attachment_pixel_format, "StencilFormat")?);
        }
        if d.sample_count > 1 {
            rpd.raster_sample_count = d.sample_count as u32;
        }
        if (d.vertex_attribute_count > 0 && !d.vertex_attributes.is_null())
            || (d.vertex_layout_count > 0 && !d.vertex_layouts.is_null())
        {
            let mut vd = VertexDescriptor::new();
            if d.vertex_attribute_count > 0 && !d.vertex_attributes.is_null() {
                let attrs = unsafe {
                    std::slice::from_raw_parts(d.vertex_attributes, d.vertex_attribute_count as usize)
                };
                for a in attrs {
                    let fmt: VertexFormat = raw_enum(a.format, "VertexFormat")?;
                    vd.attributes.insert(
                        a.index,
                        VertexAttributeDescriptor::new(fmt, a.offset, a.buffer_index),
                    );
                }
            }
            if d.vertex_layout_count > 0 && !d.vertex_layouts.is_null() {
                let layouts = unsafe {
                    std::slice::from_raw_parts(d.vertex_layouts, d.vertex_layout_count as usize)
                };
                for l in layouts {
                    let stride = if l.dynamic_stride {
                        None
                    } else {
                        Some(l.stride as u32)
                    };
                    let mut ld = VertexBufferLayoutDescriptor::new(stride);
                    ld.step_function = raw_enum::<VertexStepFunction>(l.step_function, "VertexStepFunction")?;
                    ld.step_rate = l.step_rate;
                    vd.layouts.insert(l.index, ld);
                }
            }
            rpd.vertex_descriptor = Some(vd);
        }
        let pso = dev
            .new_render_pipeline_state(&rpd, &PipelineOptions::default())
            .map_err(|e| format!("render pipeline: {e:?}"))?;
        Ok(boxed(pso))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_pipeline_release(pso: *mut RenderPipelineState) {
    release(pso);
}

#[no_mangle]
pub extern "C" fn xm2v_sampler_new(
    dev: *mut Device,
    desc: *const Xm2vSamplerDesc,
) -> *mut SamplerState {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let d = get(desc)?;
        let mut sd = SamplerDescriptor::default();
        sd.min_filter = raw_enum::<SamplerMinMagFilter>(d.min_filter, "MinFilter")?;
        sd.mag_filter = raw_enum::<SamplerMinMagFilter>(d.mag_filter, "MagFilter")?;
        sd.mip_filter = raw_enum::<SamplerMipFilter>(d.mip_filter, "MipFilter")?;
        sd.s_address_mode = raw_enum::<SamplerAddressMode>(d.s_address_mode, "SAddressMode")?;
        sd.t_address_mode = raw_enum::<SamplerAddressMode>(d.t_address_mode, "TAddressMode")?;
        sd.r_address_mode = raw_enum::<SamplerAddressMode>(d.r_address_mode, "RAddressMode")?;
        sd.max_anisotropy = (d.max_anisotropy.max(1)) as u32;
        sd.compare_function = raw_enum::<CompareFunction>(d.compare_function, "CompareFunction")?;
        sd.lod_min_clamp = d.lod_min_clamp;
        sd.lod_max_clamp = d.lod_max_clamp;
        sd.normalized_coordinates = d.normalized_coordinates;
        let s = dev
            .new_sampler_state(&sd)
            .map_err(|e| format!("new_sampler_state: {e:?}"))?;
        Ok(boxed(s))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_sampler_release(s: *mut SamplerState) {
    release(s);
}

#[no_mangle]
pub extern "C" fn xm2v_sampler_gpu_resource_id(s: *const SamplerState) -> u64 {
    guard(0, || Ok(get(s)?.gpu_resource_id().0))
}

#[no_mangle]
pub extern "C" fn xm2v_depth_stencil_new(
    dev: *mut Device,
    desc: *const Xm2vDepthStencilDesc,
) -> *mut DepthStencilState {
    guard(std::ptr::null_mut(), || {
        let dev = get(dev)?;
        let d = get(desc)?;
        let mut dsd = DepthStencilDescriptor::default();
        dsd.depth_compare_function =
            raw_enum::<CompareFunction>(d.depth_compare_function, "CompareFunction")?;
        dsd.depth_write_enabled = d.depth_write_enabled;
        let map_stencil = |s: &Xm2vStencilDesc| -> Result<StencilDescriptor, String> {
            let mut sd = StencilDescriptor::default();
            if !s.enabled {
                return Ok(sd);
            }
            sd.stencil_compare_function =
                raw_enum::<CompareFunction>(s.stencil_compare_function, "StencilCompareFunction")?;
            sd.stencil_failure_operation =
                raw_enum::<StencilOperation>(s.stencil_failure_operation, "StencilFailureOp")?;
            sd.depth_failure_operation =
                raw_enum::<StencilOperation>(s.depth_failure_operation, "DepthFailureOp")?;
            sd.depth_stencil_pass_operation = raw_enum::<StencilOperation>(
                s.depth_stencil_pass_operation,
                "DepthStencilPassOp",
            )?;
            sd.read_mask = s.read_mask;
            sd.write_mask = s.write_mask;
            Ok(sd)
        };
        dsd.front_face_stencil = map_stencil(&d.front_face_stencil)?;
        dsd.back_face_stencil = map_stencil(&d.back_face_stencil)?;
        let ds = dev
            .new_depth_stencil_state(&dsd)
            .map_err(|e| format!("new_depth_stencil_state: {e:?}"))?;
        Ok(boxed(ds))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_depth_stencil_release(ds: *mut DepthStencilState) {
    release(ds);
}

/* ---- Queue & CommandBuffer ---- */

#[no_mangle]
pub extern "C" fn xm2v_queue_new(dev: *mut Device) -> *mut CommandQueue {
    guard(std::ptr::null_mut(), || {
        let q = get(dev)?
            .new_command_queue()
            .map_err(|e| format!("new_command_queue: {e:?}"))?;
        Ok(boxed(q))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_queue_release(q: *mut CommandQueue) {
    release(q);
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_new(q: *mut CommandQueue) -> *mut CommandBuffer {
    guard(std::ptr::null_mut(), || {
        let cb = get(q)?
            .command_buffer()
            .map_err(|e| format!("command_buffer: {e:?}"))?;
        Ok(boxed(cb))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_release(cb: *mut CommandBuffer) {
    release(cb);
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_commit(cb: *mut CommandBuffer) -> bool {
    guard(false, || {
        get(cb)?
            .commit()
            .map_err(|e| format!("commit: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_wait_until_completed(
    cb: *mut CommandBuffer,
    timeout_ms: u64,
) -> bool {
    guard(false, || {
        let cb = get(cb)?;
        if timeout_ms == 0 {
            let _ = cb.wait_until_completed();
        } else {
            cb.wait_until_completed_for(Duration::from_millis(timeout_ms))
                .map_err(|e| format!("wait_until_completed: {e:?}"))?;
        }
        Ok(cb.status() == CommandBufferStatus::Completed)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_status(cb: *const CommandBuffer) -> u32 {
    guard(5, || {
        Ok(match get(cb)?.status() {
            CommandBufferStatus::NotEnqueued => 0,
            CommandBufferStatus::Enqueued => 1,
            CommandBufferStatus::Committed => 2,
            CommandBufferStatus::Scheduled => 3,
            CommandBufferStatus::Completed => 4,
            CommandBufferStatus::Error => 5,
            _ => 5,
        })
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_error(
    cb: *const CommandBuffer,
    out: *mut c_char,
    size: usize,
) -> usize {
    guard(0, || {
        let cb = get(cb)?;
        let mut parts: Vec<String> = Vec::new();
        if let Some(err) = cb.error() {
            parts.push(format!("{err:?}"));
        }
        if let Some(refusal) = cb.refusal() {
            parts.push(format!("refusal={refusal:?}"));
        }
        if let Some(desc) = cb.error_description() {
            if !desc.is_empty() {
                parts.push(desc);
            }
        }
        if parts.is_empty() {
            return Ok(0);
        }
        Ok(copy_str(&parts.join(": "), out, size))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_encode_signal_event(
    cb: *mut CommandBuffer,
    ev: *mut Event,
    value: u64,
) -> bool {
    guard(false, || {
        get(cb)?
            .encode_signal_event(EventRef::Event(get(ev)?), value)
            .map_err(|e| format!("encode_signal_event: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_encode_signal_event_scheduled(
    cb: *mut CommandBuffer,
    ev: *mut Event,
    value: u64,
) -> bool {
    guard(false, || {
        get(cb)?
            .encode_signal_event(EventRef::Event(get(ev)?), value)
            .map_err(|e| format!("encode_signal_event_scheduled: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_encode_wait_event(
    cb: *mut CommandBuffer,
    ev: *mut Event,
    value: u64,
) -> bool {
    guard(false, || {
        get(cb)?
            .encode_wait_for_event(EventRef::Event(get(ev)?), value)
            .map_err(|e| format!("encode_wait_for_event: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_encode_signal_shared_event(
    cb: *mut CommandBuffer,
    ev: *mut SharedEvent,
    value: u64,
) -> bool {
    guard(false, || {
        get(cb)?
            .encode_signal_event(EventRef::Shared(get(ev)?), value)
            .map_err(|e| format!("encode_signal_shared_event: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_encode_signal_shared_event_scheduled(
    cb: *mut CommandBuffer,
    ev: *mut SharedEvent,
    value: u64,
) -> bool {
    guard(false, || {
        get(cb)?
            .encode_signal_event_scheduled(get(ev)?, value)
            .map_err(|e| format!("encode_signal_shared_event_scheduled: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_command_buffer_encode_wait_shared_event(
    cb: *mut CommandBuffer,
    ev: *mut SharedEvent,
    value: u64,
) -> bool {
    guard(false, || {
        get(cb)?
            .encode_wait_for_event(EventRef::Shared(get(ev)?), value)
            .map_err(|e| format!("encode_wait_shared_event: {e:?}"))?;
        Ok(true)
    })
}

/* ---- Compute Encoder ---- */

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_new(cb: *mut CommandBuffer) -> *mut ComputeCommandEncoder {
    guard(std::ptr::null_mut(), || {
        let enc = get(cb)?
            .compute_command_encoder()
            .map_err(|e| format!("compute_command_encoder: {e:?}"))?;
        Ok(boxed(enc))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_set_pipeline(
    enc: *mut ComputeCommandEncoder,
    pso: *mut ComputePipelineState,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .set_compute_pipeline_state(get(pso)?)
            .map_err(|e| format!("set_compute_pipeline_state: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_set_buffer(
    enc: *mut ComputeCommandEncoder,
    buf: *mut Buffer,
    offset: u64,
    index: u32,
) -> bool {
    guard(false, || {
        let b = if buf.is_null() { None } else { Some(get(buf)?) };
        get_mut(enc)?
            .set_buffer(b, offset, index)
            .map_err(|e| format!("set_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_set_bytes(
    enc: *mut ComputeCommandEncoder,
    bytes: *const c_void,
    len: usize,
    index: u32,
) -> bool {
    guard(false, || {
        let slice = if bytes.is_null() || len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) }
        };
        get_mut(enc)?
            .set_bytes(slice, index)
            .map_err(|e| format!("set_bytes: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_set_texture(
    enc: *mut ComputeCommandEncoder,
    tex: *mut Texture,
    index: u32,
) -> bool {
    guard(false, || {
        let t = if tex.is_null() { None } else { Some(get(tex)?) };
        get_mut(enc)?
            .set_texture(t, index)
            .map_err(|e| format!("set_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_set_sampler(
    enc: *mut ComputeCommandEncoder,
    samp: *mut SamplerState,
    index: u32,
) -> bool {
    guard(false, || {
        let s = if samp.is_null() { None } else { Some(get(samp)?) };
        get_mut(enc)?
            .set_sampler_state(s, index)
            .map_err(|e| format!("set_sampler_state: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_set_threadgroup_memory_length(
    enc: *mut ComputeCommandEncoder,
    length: u32,
    index: u32,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .set_threadgroup_memory_length(length, index)
            .map_err(|e| format!("set_threadgroup_memory_length: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_use_buffer(
    enc: *mut ComputeCommandEncoder,
    buf: *mut Buffer,
    usage: u64,
) -> bool {
    guard(false, || {
        let u: ResourceUsage = raw_enum(usage, "ResourceUsage")?;
        get_mut(enc)?
            .use_resource(ResourceRef::Buffer(get(buf)?), u)
            .map_err(|e| format!("compute use_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_use_texture(
    enc: *mut ComputeCommandEncoder,
    tex: *mut Texture,
    usage: u64,
) -> bool {
    guard(false, || {
        let u: ResourceUsage = raw_enum(usage, "ResourceUsage")?;
        get_mut(enc)?
            .use_resource(ResourceRef::Texture(get(tex)?), u)
            .map_err(|e| format!("compute use_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_dispatch_threadgroups(
    enc: *mut ComputeCommandEncoder,
    gx: u64,
    gy: u64,
    gz: u64,
    tx: u64,
    ty: u64,
    tz: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .dispatch_threadgroups(Size::new(gx, gy, gz), Size::new(tx, ty, tz))
            .map_err(|e| format!("dispatch_threadgroups: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_dispatch_threads(
    enc: *mut ComputeCommandEncoder,
    gx: u64,
    gy: u64,
    gz: u64,
    tx: u64,
    ty: u64,
    tz: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .dispatch_threads(Size::new(gx, gy, gz), Size::new(tx, ty, tz))
            .map_err(|e| format!("dispatch_threads: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_update_fence(
    enc: *mut ComputeCommandEncoder,
    fence: *mut Fence,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .update_fence(get(fence)?)
            .map_err(|e| format!("compute update_fence: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_wait_fence(
    enc: *mut ComputeCommandEncoder,
    fence: *mut Fence,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .wait_for_fence(get(fence)?)
            .map_err(|e| format!("compute wait_for_fence: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_compute_encoder_end(enc: *mut ComputeCommandEncoder) -> bool {
    guard(false, || {
        take_box(enc)?
            .end_encoding()
            .map_err(|e| format!("compute end_encoding: {e:?}"))?;
        Ok(true)
    })
}

/* ---- Render Encoder ---- */

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_new(
    cb: *mut CommandBuffer,
    desc: *const Xm2vRenderPassDesc,
) -> *mut RenderCommandEncoder {
    guard(std::ptr::null_mut(), || {
        let cb = get(cb)?;
        let d = get(desc)?;
        let mut rpd = RenderPassDescriptor::new();
        let count = (d.color_attachment_count as usize).min(8);
        for i in 0..count {
            let ca = &d.color_attachments[i];
            if !ca.texture.is_null() {
                let att = &mut rpd.color_attachments[i];
                att.texture = Some(get(ca.texture)?.clone());
                att.level = ca.level;
                att.slice = ca.slice;
                att.load_action = raw_load(ca.load_action)?;
                att.store_action = raw_store(ca.store_action)?;
                att.clear_color = ClearColor::new(ca.clear_r, ca.clear_g, ca.clear_b, ca.clear_a);
            }
        }
        if !d.depth_texture.is_null() {
            rpd.depth_attachment.texture = Some(get(d.depth_texture)?.clone());
            rpd.depth_attachment.load_action = raw_load(d.depth_load_action)?;
            rpd.depth_attachment.store_action = raw_store(d.depth_store_action)?;
            rpd.depth_attachment.clear_depth = d.clear_depth;
        }
        if !d.stencil_texture.is_null() {
            rpd.stencil_attachment.texture = Some(get(d.stencil_texture)?.clone());
            rpd.stencil_attachment.load_action = raw_load(d.stencil_load_action)?;
            rpd.stencil_attachment.store_action = raw_store(d.stencil_store_action)?;
            rpd.stencil_attachment.clear_stencil = d.clear_stencil;
        }
        let enc = cb
            .render_command_encoder(&rpd)
            .map_err(|e| format!("render_command_encoder: {e:?}"))?;
        Ok(boxed(enc))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_pipeline(
    enc: *mut RenderCommandEncoder,
    pso: *mut RenderPipelineState,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .set_render_pipeline_state(get(pso)?)
            .map_err(|e| format!("set_render_pipeline_state: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_depth_stencil(
    enc: *mut RenderCommandEncoder,
    ds: *mut DepthStencilState,
) -> bool {
    guard(false, || {
        let s = if ds.is_null() { None } else { Some(get(ds)?) };
        get_mut(enc)?
            .set_depth_stencil_state(s)
            .map_err(|e| format!("set_depth_stencil_state: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_stencil_reference_values(
    enc: *mut RenderCommandEncoder,
    front: u32,
    back: u32,
) -> bool {
    guard(false, || {
        get_mut(enc)?.set_stencil_reference_values(front, back);
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_blend_color(
    enc: *mut RenderCommandEncoder,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> bool {
    guard(false, || {
        get_mut(enc)?.set_blend_color(r, g, b, a);
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_viewport(
    enc: *mut RenderCommandEncoder,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    znear: f64,
    zfar: f64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .set_viewport(Viewport {
                origin_x: x,
                origin_y: y,
                width: w,
                height: h,
                znear,
                zfar,
            })
            .map_err(|e| format!("set_viewport: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_scissor(
    enc: *mut RenderCommandEncoder,
    x: u64,
    y: u64,
    w: u64,
    h: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .set_scissor_rect(ScissorRect {
                x,
                y,
                width: w,
                height: h,
            })
            .map_err(|e| format!("set_scissor_rect: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_cull_mode(
    enc: *mut RenderCommandEncoder,
    mode: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?.set_cull_mode(raw_enum::<CullMode>(mode, "CullMode")?);
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_front_facing_winding(
    enc: *mut RenderCommandEncoder,
    winding: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?.set_front_facing_winding(raw_enum::<Winding>(winding, "Winding")?);
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_vertex_buffer(
    enc: *mut RenderCommandEncoder,
    buf: *mut Buffer,
    offset: u64,
    index: u32,
) -> bool {
    guard(false, || {
        let b = if buf.is_null() { None } else { Some(get(buf)?) };
        get_mut(enc)?
            .set_vertex_buffer(b, offset, index)
            .map_err(|e| format!("set_vertex_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_vertex_bytes(
    enc: *mut RenderCommandEncoder,
    bytes: *const c_void,
    len: usize,
    index: u32,
) -> bool {
    guard(false, || {
        let slice = if bytes.is_null() || len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) }
        };
        get_mut(enc)?
            .set_vertex_bytes(slice, index)
            .map_err(|e| format!("set_vertex_bytes: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_vertex_texture(
    enc: *mut RenderCommandEncoder,
    tex: *mut Texture,
    index: u32,
) -> bool {
    guard(false, || {
        let t = if tex.is_null() { None } else { Some(get(tex)?) };
        get_mut(enc)?
            .set_vertex_texture(t, index)
            .map_err(|e| format!("set_vertex_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_vertex_sampler(
    enc: *mut RenderCommandEncoder,
    samp: *mut SamplerState,
    index: u32,
) -> bool {
    guard(false, || {
        let s = if samp.is_null() { None } else { Some(get(samp)?) };
        get_mut(enc)?
            .set_vertex_sampler_state(s, index)
            .map_err(|e| format!("set_vertex_sampler_state: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_fragment_buffer(
    enc: *mut RenderCommandEncoder,
    buf: *mut Buffer,
    offset: u64,
    index: u32,
) -> bool {
    guard(false, || {
        let b = if buf.is_null() { None } else { Some(get(buf)?) };
        get_mut(enc)?
            .set_fragment_buffer(b, offset, index)
            .map_err(|e| format!("set_fragment_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_fragment_bytes(
    enc: *mut RenderCommandEncoder,
    bytes: *const c_void,
    len: usize,
    index: u32,
) -> bool {
    guard(false, || {
        let slice = if bytes.is_null() || len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) }
        };
        get_mut(enc)?
            .set_fragment_bytes(slice, index)
            .map_err(|e| format!("set_fragment_bytes: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_fragment_texture(
    enc: *mut RenderCommandEncoder,
    tex: *mut Texture,
    index: u32,
) -> bool {
    guard(false, || {
        let t = if tex.is_null() { None } else { Some(get(tex)?) };
        get_mut(enc)?
            .set_fragment_texture(t, index)
            .map_err(|e| format!("set_fragment_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_vertex_sampler_lod(
    enc: *mut RenderCommandEncoder,
    samp: *mut SamplerState,
    lod_min: f32,
    lod_max: f32,
    index: u32,
) -> bool {
    guard(false, || {
        let s = if samp.is_null() { None } else { Some(get(samp)?) };
        let mins = [lod_min];
        let maxs = [lod_max];
        get_mut(enc)?
            .set_vertex_sampler_states(&[s], Some((&mins, &maxs)), index)
            .map_err(|e| format!("set_vertex_sampler_states(lod): {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_fragment_sampler(
    enc: *mut RenderCommandEncoder,
    samp: *mut SamplerState,
    index: u32,
) -> bool {
    guard(false, || {
        let s = if samp.is_null() { None } else { Some(get(samp)?) };
        get_mut(enc)?
            .set_fragment_sampler_state(s, index)
            .map_err(|e| format!("set_fragment_sampler_state: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_set_fragment_sampler_lod(
    enc: *mut RenderCommandEncoder,
    samp: *mut SamplerState,
    lod_min: f32,
    lod_max: f32,
    index: u32,
) -> bool {
    guard(false, || {
        let s = if samp.is_null() { None } else { Some(get(samp)?) };
        let mins = [lod_min];
        let maxs = [lod_max];
        get_mut(enc)?
            .set_fragment_sampler_states(&[s], Some((&mins, &maxs)), index)
            .map_err(|e| format!("set_fragment_sampler_states(lod): {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_use_buffer(
    enc: *mut RenderCommandEncoder,
    buf: *mut Buffer,
    usage: u64,
    stages: u64,
) -> bool {
    guard(false, || {
        let u: ResourceUsage = raw_enum(usage, "ResourceUsage")?;
        let st: RenderStages = raw_enum(stages, "RenderStages")?;
        get_mut(enc)?
            .use_resource(ResourceRef::Buffer(get(buf)?), u, st)
            .map_err(|e| format!("render use_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_use_texture(
    enc: *mut RenderCommandEncoder,
    tex: *mut Texture,
    usage: u64,
    stages: u64,
) -> bool {
    guard(false, || {
        let u: ResourceUsage = raw_enum(usage, "ResourceUsage")?;
        let st: RenderStages = raw_enum(stages, "RenderStages")?;
        get_mut(enc)?
            .use_resource(ResourceRef::Texture(get(tex)?), u, st)
            .map_err(|e| format!("render use_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_draw_primitives(
    enc: *mut RenderCommandEncoder,
    prim: u64,
    vertex_start: u64,
    vertex_count: u64,
    instance_count: u64,
    base_instance: u64,
) -> bool {
    guard(false, || {
        let p: PrimitiveType = raw_enum(prim, "PrimitiveType")?;
        get_mut(enc)?
            .draw_primitives(
                p,
                vertex_start as u32,
                vertex_count as u32,
                instance_count as u32,
                base_instance as u32,
            )
            .map_err(|e| format!("draw_primitives: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_draw_indexed_primitives(
    enc: *mut RenderCommandEncoder,
    prim: u64,
    index_count: u64,
    index_type: u64,
    index_buf: *mut Buffer,
    index_offset: u64,
    instance_count: u64,
    base_vertex: i64,
    base_instance: u64,
) -> bool {
    guard(false, || {
        let p: PrimitiveType = raw_enum(prim, "PrimitiveType")?;
        let it: IndexType = raw_enum(index_type, "IndexType")?;
        get_mut(enc)?
            .draw_indexed_primitives(
                p,
                index_count as u32,
                it,
                get(index_buf)?,
                index_offset,
                instance_count as u32,
                base_vertex as i32,
                base_instance as u32,
            )
            .map_err(|e| format!("draw_indexed_primitives: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_update_fence(
    enc: *mut RenderCommandEncoder,
    fence: *mut Fence,
    after_stages: u64,
) -> bool {
    guard(false, || {
        let stages = raw_enum::<RenderStages>(after_stages, "RenderStages")?;
        get_mut(enc)?
            .update_fence(get(fence)?, stages)
            .map_err(|e| format!("render update_fence: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_wait_fence(
    enc: *mut RenderCommandEncoder,
    fence: *mut Fence,
    before_stages: u64,
) -> bool {
    guard(false, || {
        let stages = raw_enum::<RenderStages>(before_stages, "RenderStages")?;
        get_mut(enc)?
            .wait_for_fence(get(fence)?, stages)
            .map_err(|e| format!("render wait_for_fence: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_render_encoder_end(enc: *mut RenderCommandEncoder) -> bool {
    guard(false, || {
        take_box(enc)?
            .end_encoding()
            .map_err(|e| format!("render end_encoding: {e:?}"))?;
        Ok(true)
    })
}

/* ---- Blit Encoder ---- */

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_new(cb: *mut CommandBuffer) -> *mut BlitCommandEncoder {
    guard(std::ptr::null_mut(), || {
        let enc = get(cb)?
            .blit_command_encoder()
            .map_err(|e| format!("blit_command_encoder: {e:?}"))?;
        Ok(boxed(enc))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_copy_buffer_to_buffer(
    enc: *mut BlitCommandEncoder,
    src: *mut Buffer,
    src_off: u64,
    dst: *mut Buffer,
    dst_off: u64,
    size: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .copy_from_buffer_to_buffer(get(src)?, src_off, get(dst)?, dst_off, size)
            .map_err(|e| format!("copy_from_buffer_to_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_copy_buffer_to_texture(
    enc: *mut BlitCommandEncoder,
    src: *mut Buffer,
    src_off: u64,
    src_bpr: u64,
    src_bpi: u64,
    w: u64,
    h: u64,
    d: u64,
    dst: *mut Texture,
    dst_slice: u64,
    dst_level: u64,
    dx: u64,
    dy: u64,
    dz: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .copy_from_buffer_to_texture(
                get(src)?,
                src_off,
                src_bpr,
                src_bpi,
                Size::new(w, h, d),
                get(dst)?,
                dst_slice,
                dst_level,
                Origin::new(dx, dy, dz),
                BlitOption::None,
            )
            .map_err(|e| format!("copy_from_buffer_to_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_copy_texture_to_buffer(
    enc: *mut BlitCommandEncoder,
    src: *mut Texture,
    src_slice: u64,
    src_level: u64,
    sx: u64,
    sy: u64,
    sz: u64,
    w: u64,
    h: u64,
    d: u64,
    dst: *mut Buffer,
    dst_off: u64,
    dst_bpr: u64,
    dst_bpi: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .copy_from_texture_to_buffer(
                get(src)?,
                src_slice,
                src_level,
                Origin::new(sx, sy, sz),
                Size::new(w, h, d),
                get(dst)?,
                dst_off,
                dst_bpr,
                dst_bpi,
                BlitOption::None,
            )
            .map_err(|e| format!("copy_from_texture_to_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_copy_texture_to_texture(
    enc: *mut BlitCommandEncoder,
    src: *mut Texture,
    src_slice: u64,
    src_level: u64,
    sx: u64,
    sy: u64,
    sz: u64,
    w: u64,
    h: u64,
    d: u64,
    dst: *mut Texture,
    dst_slice: u64,
    dst_level: u64,
    dx: u64,
    dy: u64,
    dz: u64,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .copy_from_texture_to_texture(
                get(src)?,
                src_slice,
                src_level,
                Origin::new(sx, sy, sz),
                Size::new(w, h, d),
                get(dst)?,
                dst_slice,
                dst_level,
                Origin::new(dx, dy, dz),
            )
            .map_err(|e| format!("copy_from_texture_to_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_fill_buffer(
    enc: *mut BlitCommandEncoder,
    buf: *mut Buffer,
    location: u64,
    length: u64,
    value: u8,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .fill_buffer(get(buf)?, location..location.saturating_add(length), value)
            .map_err(|e| format!("fill_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_generate_mipmaps(
    enc: *mut BlitCommandEncoder,
    tex: *mut Texture,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .generate_mipmaps(get(tex)?)
            .map_err(|e| format!("generate_mipmaps: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_synchronize_buffer(
    enc: *mut BlitCommandEncoder,
    buf: *mut Buffer,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .synchronize_resource(ResourceRef::Buffer(get(buf)?))
            .map_err(|e| format!("synchronize_buffer: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_synchronize_texture(
    enc: *mut BlitCommandEncoder,
    tex: *mut Texture,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .synchronize_resource(ResourceRef::Texture(get(tex)?))
            .map_err(|e| format!("synchronize_texture: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_update_fence(
    enc: *mut BlitCommandEncoder,
    fence: *mut Fence,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .update_fence(get(fence)?)
            .map_err(|e| format!("blit update_fence: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_wait_fence(
    enc: *mut BlitCommandEncoder,
    fence: *mut Fence,
) -> bool {
    guard(false, || {
        get_mut(enc)?
            .wait_for_fence(get(fence)?)
            .map_err(|e| format!("blit wait_for_fence: {e:?}"))?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_blit_encoder_end(enc: *mut BlitCommandEncoder) -> bool {
    guard(false, || {
        take_box(enc)?
            .end_encoding()
            .map_err(|e| format!("blit end_encoding: {e:?}"))?;
        Ok(true)
    })
}

/* ---- Events & Fences ---- */

static SHARED_EVENT_LISTENER: OnceLock<SharedEventListener> = OnceLock::new();

#[no_mangle]
pub extern "C" fn xm2v_event_new(dev: *mut Device) -> *mut Event {
    guard(std::ptr::null_mut(), || {
        let ev = get(dev)?
            .new_event()
            .map_err(|e| format!("new_event: {e:?}"))?;
        Ok(boxed(ev))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_event_release(ev: *mut Event) {
    release(ev);
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_new(dev: *mut Device) -> *mut SharedEvent {
    guard(std::ptr::null_mut(), || {
        let ev = get(dev)?
            .new_shared_event()
            .map_err(|e| format!("new_shared_event: {e:?}"))?;
        Ok(boxed(ev))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_new_with_handle(
    dev: *mut Device,
    handle: *const SharedEventHandle,
) -> *mut SharedEvent {
    guard(std::ptr::null_mut(), || {
        let ev = get(dev)?
            .new_shared_event_with_handle(get(handle)?)
            .map_err(|e| format!("new_shared_event_with_handle: {e:?}"))?;
        Ok(boxed(ev))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_release(ev: *mut SharedEvent) {
    release(ev);
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_signaled_value(ev: *const SharedEvent) -> u64 {
    guard(0, || Ok(get(ev)?.signaled_value()))
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_set_signaled_value(ev: *const SharedEvent, value: u64) {
    let _ = guard((), || {
        get(ev)?.set_signaled_value(value);
        Ok(())
    });
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_wait_until(
    ev: *const SharedEvent,
    value: u64,
    timeout_ms: u64,
) -> bool {
    guard(false, || {
        let dur = if timeout_ms == u64::MAX {
            Duration::MAX
        } else {
            Duration::from_millis(timeout_ms)
        };
        Ok(get(ev)?.wait_until_signaled_value(value, dur))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_notify(
    ev: *const SharedEvent,
    value: u64,
    cb: Option<Xm2vSharedEventNotifyFn>,
    ctx: *mut c_void,
) -> bool {
    guard(false, || {
        let ev = get(ev)?;
        let cb = cb.ok_or_else(|| "null notify callback".to_string())?;
        let listener = SHARED_EVENT_LISTENER.get_or_init(SharedEventListener::new);
        let ctx_addr = ctx as usize;
        ev.notify_listener(
            listener,
            value,
            Box::new(move |_event, signaled| {
                cb(ctx_addr as *mut c_void, signaled);
            }),
        );
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_new_handle(ev: *const SharedEvent) -> *mut SharedEventHandle {
    guard(std::ptr::null_mut(), || {
        Ok(boxed(get(ev)?.new_shared_event_handle()))
    })
}

#[no_mangle]
pub extern "C" fn xm2v_shared_event_handle_release(h: *mut SharedEventHandle) {
    release(h);
}

#[no_mangle]
pub extern "C" fn xm2v_fence_new(dev: *mut Device) -> *mut Fence {
    guard(std::ptr::null_mut(), || Ok(boxed(get(dev)?.new_fence())))
}

#[no_mangle]
pub extern "C" fn xm2v_fence_release(f: *mut Fence) {
    release(f);
}

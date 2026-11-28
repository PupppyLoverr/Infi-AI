// SPDX-License-Identifier: GPL-2.0-or-later
#ifndef XNU_M2V_H
#define XNU_M2V_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct Xm2vDevice Xm2vDevice;
typedef struct Xm2vBuffer Xm2vBuffer;
typedef struct Xm2vTexture Xm2vTexture;
typedef struct Xm2vLibrary Xm2vLibrary;
typedef struct Xm2vFunction Xm2vFunction;
typedef struct Xm2vComputePipeline Xm2vComputePipeline;
typedef struct Xm2vRenderPipeline Xm2vRenderPipeline;
typedef struct Xm2vSampler Xm2vSampler;
typedef struct Xm2vDepthStencil Xm2vDepthStencil;
typedef struct Xm2vQueue Xm2vQueue;
typedef struct Xm2vCommandBuffer Xm2vCommandBuffer;
typedef struct Xm2vComputeEncoder Xm2vComputeEncoder;
typedef struct Xm2vRenderEncoder Xm2vRenderEncoder;
typedef struct Xm2vBlitEncoder Xm2vBlitEncoder;
typedef struct Xm2vEvent Xm2vEvent;
typedef struct Xm2vSharedEvent Xm2vSharedEvent;
typedef struct Xm2vSharedEventHandle Xm2vSharedEventHandle;
typedef struct Xm2vFence Xm2vFence;
typedef struct Xm2vArgumentEncoder Xm2vArgumentEncoder;

typedef void (*Xm2vDeallocatorFn)(void *ptr, uint64_t length, void *ctx);
typedef void (*Xm2vSharedEventNotifyFn)(void *ctx, uint64_t value);

typedef struct Xm2vTextureDesc {
    uint64_t texture_type;
    uint64_t pixel_format;
    uint64_t width;
    uint64_t height;
    uint64_t depth;
    uint64_t mipmap_level_count;
    uint64_t sample_count;
    uint64_t array_length;
    uint64_t resource_options;
    uint64_t usage;
} Xm2vTextureDesc;

typedef struct Xm2vColorAttachmentDesc {
    uint64_t pixel_format;
    bool blending_enabled;
    uint64_t rgb_blend_op;
    uint64_t alpha_blend_op;
    uint64_t src_rgb_blend;
    uint64_t dst_rgb_blend;
    uint64_t src_alpha_blend;
    uint64_t dst_alpha_blend;
    uint64_t write_mask;
} Xm2vColorAttachmentDesc;

typedef struct Xm2vVertexAttribute {
    uint32_t index;
    uint64_t format;
    uint32_t offset;
    uint32_t buffer_index;
} Xm2vVertexAttribute;

typedef struct Xm2vVertexLayout {
    uint32_t index;
    uint64_t stride;
    uint64_t step_function;
    uint32_t step_rate;
    bool dynamic_stride;
} Xm2vVertexLayout;

typedef struct Xm2vRenderPipelineDesc {
    Xm2vFunction *vertex_function;
    Xm2vFunction *fragment_function;
    uint32_t color_attachment_count;
    Xm2vColorAttachmentDesc color_attachments[8];
    uint64_t depth_attachment_pixel_format;
    uint64_t stencil_attachment_pixel_format;
    uint64_t sample_count;
    Xm2vFunction *const *vertex_linked_functions;
    uint32_t vertex_linked_function_count;
    Xm2vFunction *const *vertex_private_linked_functions;
    uint32_t vertex_private_linked_function_count;
    Xm2vFunction *const *fragment_linked_functions;
    uint32_t fragment_linked_function_count;
    Xm2vFunction *const *fragment_private_linked_functions;
    uint32_t fragment_private_linked_function_count;
    const Xm2vVertexAttribute *vertex_attributes;
    uint32_t vertex_attribute_count;
    const Xm2vVertexLayout *vertex_layouts;
    uint32_t vertex_layout_count;
} Xm2vRenderPipelineDesc;

typedef struct Xm2vRenderPassColorAttachment {
    Xm2vTexture *texture;
    uint64_t level;
    uint64_t slice;
    uint64_t load_action;
    uint64_t store_action;
    double clear_r, clear_g, clear_b, clear_a;
} Xm2vRenderPassColorAttachment;

typedef struct Xm2vRenderPassDesc {
    uint32_t color_attachment_count;
    Xm2vRenderPassColorAttachment color_attachments[8];
    Xm2vTexture *depth_texture;
    uint64_t depth_load_action;
    uint64_t depth_store_action;
    double clear_depth;
    Xm2vTexture *stencil_texture;
    uint64_t stencil_load_action;
    uint64_t stencil_store_action;
    uint32_t clear_stencil;
} Xm2vRenderPassDesc;

typedef struct Xm2vSamplerDesc {
    uint64_t min_filter;
    uint64_t mag_filter;
    uint64_t mip_filter;
    uint64_t s_address_mode;
    uint64_t t_address_mode;
    uint64_t r_address_mode;
    uint64_t max_anisotropy;
    uint64_t compare_function;
    float lod_min_clamp;
    float lod_max_clamp;
    bool normalized_coordinates;
} Xm2vSamplerDesc;

typedef struct Xm2vStencilDesc {
    bool enabled;
    uint64_t stencil_compare_function;
    uint64_t stencil_failure_operation;
    uint64_t depth_failure_operation;
    uint64_t depth_stencil_pass_operation;
    uint32_t read_mask;
    uint32_t write_mask;
} Xm2vStencilDesc;

typedef struct Xm2vDepthStencilDesc {
    uint64_t depth_compare_function;
    bool depth_write_enabled;
    Xm2vStencilDesc front_face_stencil;
    Xm2vStencilDesc back_face_stencil;
} Xm2vDepthStencilDesc;

/* Error reporting */
const char *xm2v_last_error(void);

/* Device */
Xm2vDevice *xm2v_device_open(const char *loader_path, const char *icd_json_path, const char *cache_dir);
Xm2vDevice *xm2v_device_open_direct(const char *loader_path, void *direct_driver_pfn,
                                    const char *icd_json_path, const char *cache_dir);
void xm2v_device_release(Xm2vDevice *dev);
size_t xm2v_device_name(const Xm2vDevice *dev, char *out, size_t size);
uint64_t xm2v_device_max_buffer_length(const Xm2vDevice *dev);
bool xm2v_device_max_threads_per_threadgroup(const Xm2vDevice *dev, uint64_t out[3]);
bool xm2v_device_supports_family(const Xm2vDevice *dev, int64_t family);

/* Buffers */
Xm2vBuffer *xm2v_buffer_new(Xm2vDevice *dev, uint64_t length, uint64_t resource_options, const void *bytes);
Xm2vBuffer *xm2v_buffer_new_page_aligned(Xm2vDevice *dev, uint64_t length, uint64_t resource_options, const void *bytes);
Xm2vBuffer *xm2v_buffer_new_no_copy(Xm2vDevice *dev, void *ptr, uint64_t length, uint64_t resource_options);
Xm2vBuffer *xm2v_buffer_new_no_copy_dealloc(Xm2vDevice *dev, void *ptr, uint64_t length, uint64_t resource_options,
                                            Xm2vDeallocatorFn dealloc_fn, void *dealloc_ctx);
void xm2v_buffer_release(Xm2vBuffer *buf);
void *xm2v_buffer_contents(const Xm2vBuffer *buf);
uint64_t xm2v_buffer_length(const Xm2vBuffer *buf);
uint64_t xm2v_buffer_gpu_address(const Xm2vBuffer *buf);
void xm2v_buffer_did_modify_range(const Xm2vBuffer *buf, uint64_t location, uint64_t length);
Xm2vTexture *xm2v_buffer_new_texture(Xm2vBuffer *buf, const Xm2vTextureDesc *desc, uint64_t offset, uint64_t bytes_per_row);
Xm2vTexture *xm2v_buffer_new_linear_texture(Xm2vBuffer *buf, const Xm2vTextureDesc *desc, uint64_t offset, uint64_t bytes_per_row, uint64_t bytes_per_image);
bool xm2v_pixel_format_is_ycbcr(uint64_t pixel_format);
Xm2vTexture *xm2v_buffer_new_texture_with_iosurface_ycbcr(
    Xm2vBuffer *buf,
    const Xm2vTextureDesc *desc,
    uint64_t luma_offset,
    uint64_t luma_bytes_per_row,
    bool planar,
    uint32_t surface_pixel_format,
    uint64_t surface_plane,
    uint64_t surface_width,
    uint64_t surface_height,
    uint64_t chroma_offset,
    uint64_t chroma_bytes_per_row,
    const char *ycbcr_matrix);

/* Textures */
Xm2vTexture *xm2v_texture_new(Xm2vDevice *dev, const Xm2vTextureDesc *desc);
Xm2vTexture *xm2v_texture_new_view(Xm2vTexture *tex, uint64_t pixel_format, uint64_t texture_type,
                                   uint64_t level_start, uint64_t level_len,
                                   uint64_t slice_start, uint64_t slice_len);
void xm2v_texture_release(Xm2vTexture *tex);
uint64_t xm2v_texture_gpu_resource_id(const Xm2vTexture *tex);
bool xm2v_texture_replace_region(const Xm2vTexture *tex,
                                 uint64_t x, uint64_t y, uint64_t z,
                                 uint64_t w, uint64_t h, uint64_t d,
                                 uint64_t mip_level, uint64_t slice,
                                 const void *bytes, uint64_t bytes_per_row, uint64_t bytes_per_image);
bool xm2v_texture_get_bytes(const Xm2vTexture *tex,
                            void *out_bytes, uint64_t bytes_per_row, uint64_t bytes_per_image,
                            uint64_t x, uint64_t y, uint64_t z,
                            uint64_t w, uint64_t h, uint64_t d,
                            uint64_t mip_level, uint64_t slice);

/* Libraries & Functions */
typedef struct Xm2vConstant {
    uint64_t data_type;
    uint32_t index;
    const char *name; /* NULL => indexed constant */
    const void *bytes;
    size_t length;
} Xm2vConstant;

Xm2vLibrary *xm2v_library_new_with_data(Xm2vDevice *dev, const void *bytes, size_t len);
void xm2v_library_release(Xm2vLibrary *lib);
size_t xm2v_library_function_count(const Xm2vLibrary *lib);
size_t xm2v_library_function_name_at(const Xm2vLibrary *lib, size_t index, char *out, size_t size);
size_t xm2v_library_extern_function_count(const Xm2vLibrary *lib);
size_t xm2v_library_extern_function_name_at(const Xm2vLibrary *lib, size_t index, char *out, size_t size);
Xm2vFunction *xm2v_library_new_function(Xm2vLibrary *lib, const char *name);
Xm2vFunction *xm2v_library_new_extern_function(Xm2vLibrary *lib, const char *name);
Xm2vFunction *xm2v_library_new_function_specialized(Xm2vLibrary *lib, const char *name,
                                                    const Xm2vConstant *constants, uint32_t count);
Xm2vFunction *xm2v_library_new_function_specialized_zero_rest(Xm2vLibrary *lib, const char *name,
                                                              const Xm2vConstant *constants, uint32_t count);
Xm2vFunction *xm2v_library_new_function_with_descriptor(Xm2vLibrary *lib, const char *name,
                                                        const char *specialized_name,
                                                        const Xm2vConstant *constants, uint32_t count,
                                                        uint64_t options);
void xm2v_function_release(Xm2vFunction *fn);
uint32_t xm2v_function_type(const Xm2vFunction *fn);
Xm2vArgumentEncoder *xm2v_function_new_argument_encoder(Xm2vFunction *fn, uint32_t buffer_index);

/* Argument Encoder */
void xm2v_argument_encoder_release(Xm2vArgumentEncoder *enc);
uint64_t xm2v_argument_encoder_encoded_length(const Xm2vArgumentEncoder *enc);
uint64_t xm2v_argument_encoder_alignment(const Xm2vArgumentEncoder *enc);
bool xm2v_argument_encoder_set_argument_buffer(Xm2vArgumentEncoder *enc, Xm2vBuffer *buf, uint64_t offset);
bool xm2v_argument_encoder_set_argument_buffer_array_element(Xm2vArgumentEncoder *enc, Xm2vBuffer *buf, uint64_t start_offset, uint64_t array_element);
bool xm2v_argument_encoder_set_buffer(const Xm2vArgumentEncoder *enc, Xm2vBuffer *buf, uint64_t offset, uint32_t index);
bool xm2v_argument_encoder_set_texture(const Xm2vArgumentEncoder *enc, Xm2vTexture *tex, uint32_t index);
bool xm2v_argument_encoder_set_sampler(const Xm2vArgumentEncoder *enc, Xm2vSampler *samp, uint32_t index);
bool xm2v_argument_encoder_set_compute_pipeline(const Xm2vArgumentEncoder *enc, Xm2vComputePipeline *pso, uint32_t index);
bool xm2v_argument_encoder_set_render_pipeline(const Xm2vArgumentEncoder *enc, Xm2vRenderPipeline *pso, uint32_t index);
void *xm2v_argument_encoder_constant_data(const Xm2vArgumentEncoder *enc, uint32_t index);
Xm2vArgumentEncoder *xm2v_argument_encoder_new_nested(const Xm2vArgumentEncoder *enc, uint32_t index);

/* Pipelines & States */
Xm2vComputePipeline *xm2v_compute_pipeline_new(Xm2vDevice *dev, Xm2vFunction *fn);
Xm2vComputePipeline *xm2v_compute_pipeline_new_with_linked(Xm2vDevice *dev, Xm2vFunction *fn,
                                                           Xm2vFunction *const *linked_fns, uint32_t linked_count,
                                                           Xm2vFunction *const *private_fns, uint32_t private_count);
void xm2v_compute_pipeline_release(Xm2vComputePipeline *pso);
uint64_t xm2v_compute_pipeline_max_threads(const Xm2vComputePipeline *pso);
uint64_t xm2v_compute_pipeline_thread_width(const Xm2vComputePipeline *pso);
uint64_t xm2v_compute_pipeline_static_tg_mem(const Xm2vComputePipeline *pso);

Xm2vRenderPipeline *xm2v_render_pipeline_new(Xm2vDevice *dev, const Xm2vRenderPipelineDesc *desc);
void xm2v_render_pipeline_release(Xm2vRenderPipeline *pso);

Xm2vSampler *xm2v_sampler_new(Xm2vDevice *dev, const Xm2vSamplerDesc *desc);
void xm2v_sampler_release(Xm2vSampler *s);
uint64_t xm2v_sampler_gpu_resource_id(const Xm2vSampler *s);

Xm2vDepthStencil *xm2v_depth_stencil_new(Xm2vDevice *dev, const Xm2vDepthStencilDesc *desc);
void xm2v_depth_stencil_release(Xm2vDepthStencil *ds);

/* Queue & CommandBuffer */
Xm2vQueue *xm2v_queue_new(Xm2vDevice *dev);
void xm2v_queue_release(Xm2vQueue *q);

Xm2vCommandBuffer *xm2v_command_buffer_new(Xm2vQueue *q);
void xm2v_command_buffer_release(Xm2vCommandBuffer *cb);
bool xm2v_command_buffer_commit(Xm2vCommandBuffer *cb);
bool xm2v_command_buffer_wait_until_completed(Xm2vCommandBuffer *cb, uint64_t timeout_ms);
uint32_t xm2v_command_buffer_status(const Xm2vCommandBuffer *cb);
size_t xm2v_command_buffer_error(const Xm2vCommandBuffer *cb, char *out, size_t size);
bool xm2v_command_buffer_encode_signal_event(Xm2vCommandBuffer *cb, Xm2vEvent *ev, uint64_t value);
bool xm2v_command_buffer_encode_signal_event_scheduled(Xm2vCommandBuffer *cb, Xm2vEvent *ev, uint64_t value);
bool xm2v_command_buffer_encode_wait_event(Xm2vCommandBuffer *cb, Xm2vEvent *ev, uint64_t value);
bool xm2v_command_buffer_encode_signal_shared_event(Xm2vCommandBuffer *cb, Xm2vSharedEvent *ev, uint64_t value);
bool xm2v_command_buffer_encode_signal_shared_event_scheduled(Xm2vCommandBuffer *cb, Xm2vSharedEvent *ev, uint64_t value);
bool xm2v_command_buffer_encode_wait_shared_event(Xm2vCommandBuffer *cb, Xm2vSharedEvent *ev, uint64_t value);

/* Compute Encoder */
Xm2vComputeEncoder *xm2v_compute_encoder_new(Xm2vCommandBuffer *cb);
bool xm2v_compute_encoder_set_pipeline(Xm2vComputeEncoder *enc, Xm2vComputePipeline *pso);
bool xm2v_compute_encoder_set_buffer(Xm2vComputeEncoder *enc, Xm2vBuffer *buf, uint64_t offset, uint32_t index);
bool xm2v_compute_encoder_set_bytes(Xm2vComputeEncoder *enc, const void *bytes, size_t len, uint32_t index);
bool xm2v_compute_encoder_set_texture(Xm2vComputeEncoder *enc, Xm2vTexture *tex, uint32_t index);
bool xm2v_compute_encoder_set_sampler(Xm2vComputeEncoder *enc, Xm2vSampler *samp, uint32_t index);
bool xm2v_compute_encoder_set_threadgroup_memory_length(Xm2vComputeEncoder *enc, uint32_t length, uint32_t index);
bool xm2v_compute_encoder_use_buffer(Xm2vComputeEncoder *enc, Xm2vBuffer *buf, uint64_t usage);
bool xm2v_compute_encoder_use_texture(Xm2vComputeEncoder *enc, Xm2vTexture *tex, uint64_t usage);
bool xm2v_compute_encoder_dispatch_threadgroups(Xm2vComputeEncoder *enc,
                                                uint64_t gx, uint64_t gy, uint64_t gz,
                                                uint64_t tx, uint64_t ty, uint64_t tz);
bool xm2v_compute_encoder_dispatch_threads(Xm2vComputeEncoder *enc,
                                           uint64_t gx, uint64_t gy, uint64_t gz,
                                           uint64_t tx, uint64_t ty, uint64_t tz);
bool xm2v_compute_encoder_update_fence(Xm2vComputeEncoder *enc, Xm2vFence *fence);
bool xm2v_compute_encoder_wait_fence(Xm2vComputeEncoder *enc, Xm2vFence *fence);
bool xm2v_compute_encoder_end(Xm2vComputeEncoder *enc);

/* Render Encoder */
Xm2vRenderEncoder *xm2v_render_encoder_new(Xm2vCommandBuffer *cb, const Xm2vRenderPassDesc *desc);
bool xm2v_render_encoder_set_pipeline(Xm2vRenderEncoder *enc, Xm2vRenderPipeline *pso);
bool xm2v_render_encoder_set_depth_stencil(Xm2vRenderEncoder *enc, Xm2vDepthStencil *ds);
bool xm2v_render_encoder_set_viewport(Xm2vRenderEncoder *enc, double x, double y, double w, double h, double znear, double zfar);
bool xm2v_render_encoder_set_scissor(Xm2vRenderEncoder *enc, uint64_t x, uint64_t y, uint64_t w, uint64_t h);
bool xm2v_render_encoder_set_cull_mode(Xm2vRenderEncoder *enc, uint64_t mode);
bool xm2v_render_encoder_set_front_facing_winding(Xm2vRenderEncoder *enc, uint64_t winding);
bool xm2v_render_encoder_set_stencil_reference_values(Xm2vRenderEncoder *enc, uint32_t front, uint32_t back);
bool xm2v_render_encoder_set_blend_color(Xm2vRenderEncoder *enc, float r, float g, float b, float a);
bool xm2v_render_encoder_set_vertex_buffer(Xm2vRenderEncoder *enc, Xm2vBuffer *buf, uint64_t offset, uint32_t index);
bool xm2v_render_encoder_set_vertex_bytes(Xm2vRenderEncoder *enc, const void *bytes, size_t len, uint32_t index);
bool xm2v_render_encoder_set_vertex_texture(Xm2vRenderEncoder *enc, Xm2vTexture *tex, uint32_t index);
bool xm2v_render_encoder_set_vertex_sampler(Xm2vRenderEncoder *enc, Xm2vSampler *samp, uint32_t index);
bool xm2v_render_encoder_set_vertex_sampler_lod(Xm2vRenderEncoder *enc, Xm2vSampler *samp, float lod_min, float lod_max, uint32_t index);
bool xm2v_render_encoder_set_fragment_buffer(Xm2vRenderEncoder *enc, Xm2vBuffer *buf, uint64_t offset, uint32_t index);
bool xm2v_render_encoder_set_fragment_bytes(Xm2vRenderEncoder *enc, const void *bytes, size_t len, uint32_t index);
bool xm2v_render_encoder_set_fragment_texture(Xm2vRenderEncoder *enc, Xm2vTexture *tex, uint32_t index);
bool xm2v_render_encoder_set_fragment_sampler(Xm2vRenderEncoder *enc, Xm2vSampler *samp, uint32_t index);
bool xm2v_render_encoder_set_fragment_sampler_lod(Xm2vRenderEncoder *enc, Xm2vSampler *samp, float lod_min, float lod_max, uint32_t index);
bool xm2v_render_encoder_use_buffer(Xm2vRenderEncoder *enc, Xm2vBuffer *buf, uint64_t usage, uint64_t stages);
bool xm2v_render_encoder_use_texture(Xm2vRenderEncoder *enc, Xm2vTexture *tex, uint64_t usage, uint64_t stages);
bool xm2v_render_encoder_draw_primitives(Xm2vRenderEncoder *enc, uint64_t prim, uint64_t vertex_start, uint64_t vertex_count,
                                         uint64_t instance_count, uint64_t base_instance);
bool xm2v_render_encoder_draw_indexed_primitives(Xm2vRenderEncoder *enc, uint64_t prim, uint64_t index_count, uint64_t index_type,
                                                 Xm2vBuffer *index_buf, uint64_t index_offset, uint64_t instance_count,
                                                 int64_t base_vertex, uint64_t base_instance);
bool xm2v_render_encoder_update_fence(Xm2vRenderEncoder *enc, Xm2vFence *fence, uint64_t after_stages);
bool xm2v_render_encoder_wait_fence(Xm2vRenderEncoder *enc, Xm2vFence *fence, uint64_t before_stages);
bool xm2v_render_encoder_end(Xm2vRenderEncoder *enc);

/* Blit Encoder */
Xm2vBlitEncoder *xm2v_blit_encoder_new(Xm2vCommandBuffer *cb);
bool xm2v_blit_encoder_copy_buffer_to_buffer(Xm2vBlitEncoder *enc, Xm2vBuffer *src, uint64_t src_off,
                                             Xm2vBuffer *dst, uint64_t dst_off, uint64_t size);
bool xm2v_blit_encoder_copy_buffer_to_texture(Xm2vBlitEncoder *enc, Xm2vBuffer *src, uint64_t src_off,
                                              uint64_t src_bpr, uint64_t src_bpi,
                                              uint64_t w, uint64_t h, uint64_t d,
                                              Xm2vTexture *dst, uint64_t dst_slice, uint64_t dst_level,
                                              uint64_t dx, uint64_t dy, uint64_t dz);
bool xm2v_blit_encoder_copy_texture_to_buffer(Xm2vBlitEncoder *enc, Xm2vTexture *src, uint64_t src_slice, uint64_t src_level,
                                              uint64_t sx, uint64_t sy, uint64_t sz,
                                              uint64_t w, uint64_t h, uint64_t d,
                                              Xm2vBuffer *dst, uint64_t dst_off, uint64_t dst_bpr, uint64_t dst_bpi);
bool xm2v_blit_encoder_copy_texture_to_texture(Xm2vBlitEncoder *enc,
                                               Xm2vTexture *src, uint64_t src_slice, uint64_t src_level,
                                               uint64_t sx, uint64_t sy, uint64_t sz,
                                               uint64_t w, uint64_t h, uint64_t d,
                                               Xm2vTexture *dst, uint64_t dst_slice, uint64_t dst_level,
                                               uint64_t dx, uint64_t dy, uint64_t dz);
bool xm2v_blit_encoder_fill_buffer(Xm2vBlitEncoder *enc, Xm2vBuffer *buf, uint64_t location, uint64_t length, uint8_t value);
bool xm2v_blit_encoder_generate_mipmaps(Xm2vBlitEncoder *enc, Xm2vTexture *tex);
bool xm2v_blit_encoder_synchronize_buffer(Xm2vBlitEncoder *enc, Xm2vBuffer *buf);
bool xm2v_blit_encoder_synchronize_texture(Xm2vBlitEncoder *enc, Xm2vTexture *tex);
bool xm2v_blit_encoder_update_fence(Xm2vBlitEncoder *enc, Xm2vFence *fence);
bool xm2v_blit_encoder_wait_fence(Xm2vBlitEncoder *enc, Xm2vFence *fence);
bool xm2v_blit_encoder_end(Xm2vBlitEncoder *enc);

/* Events & Fences */
Xm2vEvent *xm2v_event_new(Xm2vDevice *dev);
void xm2v_event_release(Xm2vEvent *ev);
Xm2vSharedEvent *xm2v_shared_event_new(Xm2vDevice *dev);
Xm2vSharedEvent *xm2v_shared_event_new_with_handle(Xm2vDevice *dev, const Xm2vSharedEventHandle *handle);
void xm2v_shared_event_release(Xm2vSharedEvent *ev);
uint64_t xm2v_shared_event_signaled_value(const Xm2vSharedEvent *ev);
void xm2v_shared_event_set_signaled_value(const Xm2vSharedEvent *ev, uint64_t value);
bool xm2v_shared_event_wait_until(const Xm2vSharedEvent *ev, uint64_t value, uint64_t timeout_ms);
bool xm2v_shared_event_notify(const Xm2vSharedEvent *ev, uint64_t value, Xm2vSharedEventNotifyFn cb, void *ctx);
Xm2vSharedEventHandle *xm2v_shared_event_new_handle(const Xm2vSharedEvent *ev);
void xm2v_shared_event_handle_release(Xm2vSharedEventHandle *h);
Xm2vFence *xm2v_fence_new(Xm2vDevice *dev);
void xm2v_fence_release(Xm2vFence *f);

#ifdef __cplusplus
}
#endif

#endif /* XNU_M2V_H */

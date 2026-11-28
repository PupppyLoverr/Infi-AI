// SPDX-License-Identifier: GPL-2.0-only
/*
 * Backend for a render node that is not virtio-gpu: the clients' virtio-gpu ioctls are served by libvirglrenderer
 * (Venus on the host's Vulkan driver). A client is a context, a GEM handle a resource id, an out fence a context fence.
 * Needs virglrenderer 1.2+ with -Dvenus=true and its virgl_render_server.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>
#include <drm/drm.h>
#include <drm/virtgpu_drm.h>
#include <virglrenderer.h>
#include "xnudrm_proto.h"
#include "xnudrmd.h"

#define CAPSET_VENUS 4	/* VIRTGPU_DRM_CAPSET_VENUS; older kernel headers do not name it */
#define VG_CLIENTS 256
#define VG_RES 65536
#define VG_FENCES 65536

static uint8_t clients[VG_CLIENTS];	/* 0 free, 1 open, 2 with a context (its id is the client's) */
static struct res { uint32_t client; uint32_t blob_mem; uint64_t size; void *va; uint64_t maplen; int pinned; } res[VG_RES];
/*
 * A client names a fence by its slot. The renderer's id is the slot and a serial number: a fence closed before it
 * signalled must not signal the slot's next one.
 */
static struct fence { uint32_t client; uint8_t state; uint64_t serial; } fences[VG_FENCES];	/* 0 free, 1 pending, 2 signalled */
static uint64_t fence_serial;
#define FENCE_ID(slot, serial) (((serial) << 16) | (slot))
static void *cmdbuf;

static int vgerr(int r) { return r < 0 ? r : r > 0 ? -r : 0; }

static void write_context_fence(void *cookie, uint32_t ctx, uint32_t ring, uint64_t id)
{
	struct fence *f = &fences[id & (VG_FENCES - 1)];

	if (f->state == 1 && f->client == ctx && FENCE_ID(id & (VG_FENCES - 1), f->serial) == id)
		f->state = 2;
}
static void write_fence(void *cookie, uint32_t fence) { }

static struct virgl_renderer_callbacks cbs = {
	.version = 3,
	.write_fence = write_fence,
	.write_context_fence = write_context_fence,
};

int vg_init(void)
{
	const rlim_t want = VG_RES + 2 * VG_CLIENTS + 64;
	struct rlimit rl;
	uint32_t ver = 0, size = 0;
	int r;

	/*
	 * Every resource is a descriptor here (its memory's) and every client two, and a service starts with 1024 of them.
	 * Before the render server starts: its processes get the same, and there a Vulkan driver takes what it takes for
	 * each allocation (lavapipe: three descriptors).
	 */
	if (!getrlimit(RLIMIT_NOFILE, &rl) && rl.rlim_cur < want) {
		rl.rlim_cur = rl.rlim_max < want ? rl.rlim_max : want;
		setrlimit(RLIMIT_NOFILE, &rl);
	}
	r = virgl_renderer_init(&cbs, VIRGL_RENDERER_VENUS | VIRGL_RENDERER_RENDER_SERVER | VIRGL_RENDERER_NO_VIRGL, &cbs);
	if (r) { fprintf(stderr, "xnudrmd: virgl_renderer_init: %d\n", r); return -1; }
	virgl_renderer_get_cap_set(CAPSET_VENUS, &ver, &size);
	if (!size) { fprintf(stderr, "xnudrmd: virglrenderer has no Venus capset\n"); return -1; }
	cmdbuf = malloc(XNUDRM_MAX_MSG);
	return cmdbuf ? 0 : -1;
}

static void res_free(uint32_t id)
{
	if (verbose) fprintf(stderr, "gem close: resource %u of client %u, %llu bytes%s\n", id, res[id].client,
	    (unsigned long long)res[id].size, res[id].va ? ", mapped" : "");
	if (res[id].pinned) xnudrmd_unpin(res[id].va);
	if (res[id].va) virgl_renderer_resource_unmap(id);
	if (clients[res[id].client] == 2) virgl_renderer_ctx_detach_resource(res[id].client, id);
	virgl_renderer_resource_unref(id);
	res[id] = (struct res){ 0 };
}

/* Context first, then its resources: detaching a ring's buffer from a live context is fatal to the renderer. */
static void drop_client(uint32_t c)
{
	uint32_t i;
	if (clients[c] == 2) virgl_renderer_context_destroy(c);
	clients[c] = 1;
	for (i = 1; i < VG_RES; i++) if (res[i].client == c) res_free(i);
	for (i = 1; i < VG_FENCES; i++) if (fences[i].client == c) fences[i] = (struct fence){ 0 };
	clients[c] = 0;
}

static int ptr_field(uint32_t plen, uint64_t off, uint64_t len)
{
	return off > plen || len > plen - off ? -EINVAL : 0;
}

static int do_ioctl(uint32_t c, uint32_t nr, uint8_t *p, uint32_t plen)
{
	int r;
#define NEED(type) do { if (plen < sizeof(type)) return -EINVAL; } while (0)
	switch (nr) {
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GETPARAM: {
		struct drm_virtgpu_getparam *a = (void *)p; NEED(*a);
		int v;
		if (ptr_field(plen, a->value, sizeof(int))) return -EINVAL;
		switch (a->param) {
		case VIRTGPU_PARAM_3D_FEATURES:
		case VIRTGPU_PARAM_CAPSET_QUERY_FIX:
		case VIRTGPU_PARAM_RESOURCE_BLOB:
		case VIRTGPU_PARAM_HOST_VISIBLE:
		case VIRTGPU_PARAM_CONTEXT_INIT: v = 1; break;
		case VIRTGPU_PARAM_CROSS_DEVICE: v = 0; break;
		case VIRTGPU_PARAM_SUPPORTED_CAPSET_IDs: v = 1 << CAPSET_VENUS; break;
		default: return -EINVAL;
		}
		memcpy(p + a->value, &v, sizeof(v));
		return 0;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GET_CAPS: {
		struct drm_virtgpu_get_caps *a = (void *)p; NEED(*a);
		uint32_t ver = 0, size = 0;
		void *caps;
		if (ptr_field(plen, a->addr, a->size)) return -EINVAL;
		if (a->cap_set_id != CAPSET_VENUS) return -EINVAL;
		virgl_renderer_get_cap_set(a->cap_set_id, &ver, &size);
		if (!size || !(caps = calloc(1, size))) return -ENOMEM;
		virgl_renderer_fill_caps(a->cap_set_id, a->cap_set_ver, caps);
		memcpy(p + a->addr, caps, a->size < size ? a->size : size);
		free(caps);
		return 0;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_CONTEXT_INIT: {
		struct drm_virtgpu_context_init *a = (void *)p; NEED(*a);
		struct drm_virtgpu_context_set_param *sp;
		uint32_t i, capset = 0;
		if (ptr_field(plen, a->ctx_set_params, (uint64_t)a->num_params * sizeof(*sp))) return -EINVAL;
		if (clients[c] == 2) return -EEXIST;
		sp = (void *)(p + a->ctx_set_params);
		for (i = 0; i < a->num_params; i++) if (sp[i].param == VIRTGPU_CONTEXT_PARAM_CAPSET_ID) capset = (uint32_t)sp[i].value;
		if (capset != CAPSET_VENUS) return -EINVAL;
		r = virgl_renderer_context_create_with_flags(c, capset, 7, "xnudrmd");
		if (r) return vgerr(r);
		clients[c] = 2;
		return 0;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_CREATE_BLOB: {
		struct drm_virtgpu_resource_create_blob *a = (void *)p; NEED(*a);
		uint32_t id;
		if (ptr_field(plen, a->cmd, a->cmd_size)) return -EINVAL;
		if (clients[c] != 2 || a->blob_mem != VIRTGPU_BLOB_MEM_HOST3D || !a->size) return -EINVAL;
		for (id = 1; id < VG_RES && res[id].client; id++) ;
		if (id == VG_RES) return -ENOMEM;
		if (a->cmd_size) {
			memcpy(cmdbuf, p + a->cmd, a->cmd_size);
			if ((r = virgl_renderer_submit_cmd(cmdbuf, c, a->cmd_size / 4))) return vgerr(r);
		}
		struct virgl_renderer_resource_create_blob_args b = {
			.res_handle = id, .ctx_id = c, .blob_mem = a->blob_mem, .blob_flags = a->blob_flags,
			.blob_id = a->blob_id, .size = a->size,
		};
		/* logged before the call, which waits for the render server */
		if (verbose) fprintf(stderr, "blob: client %u resource %u, blob id %llu, %llu bytes, mem %u flags %#x\n", c, id,
		    (unsigned long long)a->blob_id, (unsigned long long)a->size, a->blob_mem, a->blob_flags);
		if ((r = virgl_renderer_resource_create_blob(&b))) return vgerr(r);
		virgl_renderer_ctx_attach_resource(c, id);
		res[id] = (struct res){ .client = c, .blob_mem = a->blob_mem, .size = a->size };
		a->bo_handle = a->res_handle = id;
		return 0;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_INFO: {
		struct drm_virtgpu_resource_info *a = (void *)p; NEED(*a);
		if (!a->bo_handle || a->bo_handle >= VG_RES || res[a->bo_handle].client != c) return -ENOENT;
		a->res_handle = a->bo_handle;
		a->size = (uint32_t)res[a->bo_handle].size;
		a->blob_mem = res[a->bo_handle].blob_mem;
		return 0;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_WAIT:
		NEED(struct drm_virtgpu_3d_wait);
		return 0;
	case 0x09: {	/* DRM_IOCTL_GEM_CLOSE */
		struct drm_gem_close *a = (void *)p; NEED(*a);
		if (!a->handle || a->handle >= VG_RES || res[a->handle].client != c) return -EINVAL;
		res_free(a->handle);
		return 0;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_EXECBUFFER: {
		struct drm_virtgpu_execbuffer *a = (void *)p; NEED(*a);
		uint32_t id = 0;
		if (a->num_in_syncobjs || a->num_out_syncobjs || (a->flags & VIRTGPU_EXECBUF_FENCE_FD_IN)) return -EOPNOTSUPP;
		if (ptr_field(plen, a->command, a->size)) return -EINVAL;
		if (clients[c] != 2) return -EINVAL;
		a->fence_fd = -1;
		if (a->flags & VIRTGPU_EXECBUF_FENCE_FD_OUT) {
			for (id = 1; id < VG_FENCES && fences[id].state; id++) ;
			if (id == VG_FENCES) return -EMFILE;
		}
		if (a->size) {
			memcpy(cmdbuf, p + a->command, a->size);
			if ((r = virgl_renderer_submit_cmd(cmdbuf, c, a->size / 4))) return vgerr(r);
		}
		if (id) {
			fences[id] = (struct fence){ .client = c, .state = 1, .serial = ++fence_serial };
			r = virgl_renderer_context_create_fence(c, 0, a->flags & VIRTGPU_EXECBUF_RING_IDX ? a->ring_idx : 0,
			    FENCE_ID(id, fences[id].serial));
			if (r) { fences[id] = (struct fence){ 0 }; return vgerr(r); }
			a->fence_fd = id;	/* the fence's slot */
		}
		return 0;
	}
	default:
		return -ENOTTY;
	}
}

static int do_map(uint32_t c, struct xnudrm_hdr *h, uint8_t *out, uint32_t *outlen)
{
	uint32_t id = (uint32_t)h->arg0, nr = 0;
	struct res *rs = id && id < VG_RES ? &res[id] : NULL;
	size_t len = (h->arg1 + 4095) & ~(size_t)4095;
	int r;

	if (!rs || rs->client != c || !len) return -EINVAL;
	if (!rs->va) {
		void *va = NULL;
		uint64_t maplen = 0;
		if ((r = virgl_renderer_resource_map(id, &va, &maplen))) return vgerr(r);
		/* shared memory is pinned: unpinned, Linux could move or reclaim pages the hosted kernel has mapped */
		r = xnudrmd_pin(va, maplen);
		if (r && r != -EOPNOTSUPP) {	/* -EOPNOTSUPP: a device mapping, nothing to pin */
			fprintf(stderr, "xnudrmd: resource %u: its %llu bytes could not be pinned (%d)\n", id, (unsigned long long)maplen, r);
			virgl_renderer_resource_unmap(id);
			return r;
		}
		rs->va = va;
		rs->maplen = maplen;
		rs->pinned = !r;
	}
	if (len > ((rs->maplen + 4095) & ~(uint64_t)4095)) return -EINVAL;
	if ((r = xnudrmd_ranges(rs->va, len, (struct xnudrm_range *)out, &nr))) return r;
	h->arg0 = id;	/* the map's id */
	*outlen = nr * sizeof(struct xnudrm_range);
	if (verbose) fprintf(stderr, "map: resource %u, %zu bytes, %u ranges, first pa %#llx\n", id, len, nr, (unsigned long long)((struct xnudrm_range *)out)->pa);
	return 0;
}

static uint64_t now_ns(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec * 1000000000ull + t.tv_nsec;
}

/* No fence descriptor without the renderer's fence thread: poll. */
static int fence_wait(uint32_t c, struct fence *f, uint64_t ns)
{
	/* at most 50 ms per host call, by the clock: a usleep(100) takes half as long again */
	uint64_t end = now_ns() + (ns > 50000000 ? 50000000 : ns);

	for (;;) {
		virgl_renderer_context_poll(c);
		if (f->state == 2) return 0;
		if (now_ns() >= end) return -XNUDRM_ETIME;
		usleep(100);
	}
}

void vg_serve(const struct xnudrm_hdr *h, const uint8_t *payload, struct xnudrm_hdr *rh, uint8_t *rpayload, uint32_t *rlen)
{
	uint32_t c = h->client;
	int st = 0, open = c && c < VG_CLIENTS && clients[c];

	switch (h->op) {
	case XNUDRM_OPEN:
		if (h->arg0 != XNUDRM_VERSION) { st = -EPROTO; break; }
		for (c = 1; c < VG_CLIENTS && clients[c]; c++) ;
		if (c == VG_CLIENTS) { st = -EMFILE; break; }
		clients[c] = 1;
		rh->client = c;
		if (verbose) fprintf(stderr, "open: client %u\n", c);
		break;
	case XNUDRM_CLOSE:
		if (!open) { st = -EBADF; break; }
		drop_client(c);
		if (verbose) fprintf(stderr, "close: client %u\n", c);
		break;
	case XNUDRM_IOCTL:
		if (!open) { st = -EBADF; break; }
		memcpy(rpayload, payload, h->payload_len);
		st = do_ioctl(c, (uint32_t)h->arg0, rpayload, h->payload_len);
		*rlen = h->payload_len;
		if (verbose > 1) fprintf(stderr, "ioctl %#x: %d\n", (unsigned)h->arg0, st);
		break;
	case XNUDRM_MAP:
		if (!open) { st = -EBADF; break; }
		st = do_map(c, rh, rpayload, rlen);
		break;
	case XNUDRM_UNMAP:	/* the mapping stays until its resource goes (GEM_CLOSE) */
		if (!open || !h->arg0 || h->arg0 >= VG_RES || res[h->arg0].client != c || !res[h->arg0].va) st = -EINVAL;
		break;
	case XNUDRM_FENCE_WAIT:
	case XNUDRM_FENCE_CLOSE: {
		struct fence *f = h->arg0 && h->arg0 < VG_FENCES ? &fences[h->arg0] : NULL;
		if (!open || !f || !f->state || f->client != c) { st = -EINVAL; break; }
		if (h->op == XNUDRM_FENCE_CLOSE) *f = (struct fence){ 0 };
		else st = fence_wait(c, f, h->arg1);
		break;
	}
	default:
		st = -ENOSYS;
	}
	rh->status = st;
}

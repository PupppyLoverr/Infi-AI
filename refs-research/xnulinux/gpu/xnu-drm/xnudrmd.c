// SPDX-License-Identifier: GPL-2.0-only
/*
 * xnudrmd: the Linux end of the DRM bridge. It serves the hosted macOS's GPU clients (kexts/XnuDrm, libxnudrm) from
 * Linux's virtio-gpu render node: each request comes as a host call on /dev/xnu-hostcall (fki), is carried out with an
 * ordinary DRM ioctl, mmap or poll, and answered with the outputs. Mappings are translated into physical addresses
 * (fki's XNU_HOSTCALL_XLATE) so the hosted kernel maps the very same pages into its process: no copies.
 * A render node of another driver (the machine's own GPU) is served through virglrenderer: xnudrmd_virgl.c.
 *
 * Usage: xnudrmd [-v] [--virgl] [render node, default /dev/dri/renderD128]. Run as root (the hostcall device is 0600).
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <drm/drm.h>
#include <drm/virtgpu_drm.h>
#include "xnudrm_proto.h"
#include "xnudrmd.h"

struct xnu_hostcall_xlate { uint64_t va, len, out, max; };
#define XNU_HOSTCALL_XLATE _IOWR('X', 1, struct xnu_hostcall_xlate)
struct xnu_hostcall_range { uint64_t va, len; };
#define XNU_HOSTCALL_PIN _IOW('X', 2, struct xnu_hostcall_range)
#define XNU_HOSTCALL_UNPIN _IOW('X', 3, struct xnu_hostcall_range)

#define MAX_CLIENTS 256
#define MAX_MAPS 65536
#define MAX_FENCES 65536

static const char *node = "/dev/dri/renderD128";
static int hc = -1;
int verbose;
static int virgl;
static int clients[MAX_CLIENTS];	/* DRM fd + 1 (0: free) */
static struct map { int client; void *va; size_t len; } maps[MAX_MAPS];
static struct fence { int client; int fd; } fences[MAX_FENCES];	/* fd + 1 */

static uint8_t *msg, *rep;
#define PAYLOAD(m) ((m) + 8 + sizeof(struct xnudrm_hdr))

static int client_fd(uint32_t c) { return c && c < MAX_CLIENTS && clients[c] ? clients[c] - 1 : -1; }

/* A pointer field of an ioctl struct: an offset into the payload, checked to lie inside it with len bytes. */
static int ptr_field(uint8_t *payload, uint32_t plen, __u64 *field, uint64_t len)
{
	uint64_t off = *field;
	if (len == 0) { *field = 0; return 0; }
	if (off > plen || len > plen - off) return -EINVAL;
	*field = (uint64_t)(uintptr_t)(payload + off);
	return 0;
}
static void unptr_field(uint8_t *payload, __u64 *field)
{
	if (*field) *field = (uint64_t)((uint8_t *)(uintptr_t)*field - payload);
}

static int do_ioctl(int fd, uint32_t client, uint32_t nr, uint8_t *p, uint32_t plen)
{
	int r = 0;
#define NEED(type) do { if (plen < sizeof(type)) return -EINVAL; } while (0)
	switch (nr) {
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GETPARAM: {
		struct drm_virtgpu_getparam *a = (void *)p; NEED(*a);
		if ((r = ptr_field(p, plen, &a->value, sizeof(int)))) return r;
		r = ioctl(fd, DRM_IOCTL_VIRTGPU_GETPARAM, a);
		unptr_field(p, &a->value);
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GET_CAPS: {
		struct drm_virtgpu_get_caps *a = (void *)p; NEED(*a);
		if ((r = ptr_field(p, plen, &a->addr, a->size))) return r;
		r = ioctl(fd, DRM_IOCTL_VIRTGPU_GET_CAPS, a);
		unptr_field(p, &a->addr);
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_CONTEXT_INIT: {
		struct drm_virtgpu_context_init *a = (void *)p; NEED(*a);
		if ((r = ptr_field(p, plen, &a->ctx_set_params, (uint64_t)a->num_params * sizeof(struct drm_virtgpu_context_set_param)))) return r;
		r = ioctl(fd, DRM_IOCTL_VIRTGPU_CONTEXT_INIT, a);
		unptr_field(p, &a->ctx_set_params);
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_CREATE_BLOB: {
		struct drm_virtgpu_resource_create_blob *a = (void *)p; NEED(*a);
		if ((r = ptr_field(p, plen, &a->cmd, a->cmd_size))) return r;
		r = ioctl(fd, DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB, a);
		unptr_field(p, &a->cmd);
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_INFO: {
		NEED(struct drm_virtgpu_resource_info);
		r = ioctl(fd, DRM_IOCTL_VIRTGPU_RESOURCE_INFO, p);
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_WAIT: {
		NEED(struct drm_virtgpu_3d_wait);
		r = ioctl(fd, DRM_IOCTL_VIRTGPU_WAIT, p);
		break;
	}
	case 0x09: {	/* DRM_IOCTL_GEM_CLOSE */
		NEED(struct drm_gem_close);
		r = ioctl(fd, DRM_IOCTL_GEM_CLOSE, p);
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_EXECBUFFER: {
		struct drm_virtgpu_execbuffer *a = (void *)p; NEED(*a);
		if (a->num_in_syncobjs || a->num_out_syncobjs || (a->flags & VIRTGPU_EXECBUF_FENCE_FD_IN)) return -EOPNOTSUPP;
		if ((r = ptr_field(p, plen, &a->command, a->size))) return r;
		if ((r = ptr_field(p, plen, &a->bo_handles, (uint64_t)a->num_bo_handles * 4))) return r;
		a->fence_fd = -1;
		r = ioctl(fd, DRM_IOCTL_VIRTGPU_EXECBUFFER, a);
		unptr_field(p, &a->command);
		unptr_field(p, &a->bo_handles);
		if (r == 0 && (a->flags & VIRTGPU_EXECBUF_FENCE_FD_OUT)) {
			int i;
			for (i = 1; i < MAX_FENCES && fences[i].fd; i++) ;
			if (i == MAX_FENCES) { close(a->fence_fd); a->fence_fd = -1; return -EMFILE; }
			fences[i].fd = a->fence_fd + 1;
			fences[i].client = client;
			a->fence_fd = i;	/* the fence's id */
		}
		break;
	}
	default:
		return -ENOTTY;
	}
	return r < 0 ? -errno : 0;
}

int xnudrmd_pin(void *va, size_t len)
{
	struct xnu_hostcall_range r = { (uintptr_t)va, (len + 4095) & ~(size_t)4095 };
	return ioctl(hc, XNU_HOSTCALL_PIN, &r) ? -errno : 0;
}

void xnudrmd_unpin(void *va)
{
	struct xnu_hostcall_range r = { (uintptr_t)va, 0 };
	ioctl(hc, XNU_HOSTCALL_UNPIN, &r);
}

int xnudrmd_ranges(void *va, size_t len, struct xnudrm_range *rg, uint32_t *nr)
{
	uint32_t maxr = (XNUDRM_MAX_MSG - sizeof(struct xnudrm_hdr)) / sizeof(*rg);
	size_t i, n = len >> 12;
	uint64_t *pa;

	for (i = 0; i < n; i++) (void)*(volatile uint8_t *)((uint8_t *)va + (i << 12));	/* fault every page in */
	pa = malloc(n * 8);
	if (!pa) return -ENOMEM;
	struct xnu_hostcall_xlate x = { (uintptr_t)va, len, (uintptr_t)pa, n };
	if (ioctl(hc, XNU_HOSTCALL_XLATE, &x)) { int e = -errno; free(pa); return e; }
	*nr = 0;
	for (i = 0; i < n; i++) {
		if (*nr && rg[*nr - 1].pa + rg[*nr - 1].len == pa[i]) { rg[*nr - 1].len += 4096; continue; }
		if (*nr == maxr) { free(pa); return -E2BIG; }
		rg[*nr].pa = pa[i]; rg[*nr].len = 4096; ++*nr;
	}
	free(pa);
	return 0;
}

static int do_map(int fd, uint32_t client, struct xnudrm_hdr *h, uint8_t *out, uint32_t *outlen)
{
	struct drm_virtgpu_map m = { .handle = (uint32_t)h->arg0 };
	size_t len = (h->arg1 + 4095) & ~(size_t)4095;
	struct xnudrm_range *rg = (void *)out;
	uint32_t nr = 0;
	int id, r;
	void *va;

	if (!len) return -EINVAL;
	for (id = 1; id < MAX_MAPS && maps[id].va; id++) ;
	if (id == MAX_MAPS) return -ENOMEM;
	if (ioctl(fd, DRM_IOCTL_VIRTGPU_MAP, &m)) return -errno;
	va = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, m.offset);
	if (va == MAP_FAILED) return -errno;
	if ((r = xnudrmd_ranges(va, len, rg, &nr))) { munmap(va, len); return r; }
	maps[id] = (struct map){ client, va, len };
	h->arg0 = id;
	*outlen = nr * sizeof(*rg);
	if (verbose) fprintf(stderr, "map %d: handle %u, %zu bytes, %u ranges, first pa %#llx\n", id, m.handle, len, nr, (unsigned long long)rg[0].pa);
	return 0;
}

static void drop_client(uint32_t c)
{
	int i;
	for (i = 1; i < MAX_MAPS; i++) if (maps[i].va && maps[i].client == (int)c) { munmap(maps[i].va, maps[i].len); maps[i].va = NULL; }
	for (i = 1; i < MAX_FENCES; i++) if (fences[i].fd && fences[i].client == (int)c) { close(fences[i].fd - 1); fences[i].fd = 0; }
	if (clients[c]) close(clients[c] - 1);
	clients[c] = 0;
}

static void serve(uint8_t *in, size_t inlen, uint8_t *out, size_t *outlen)
{
	struct xnudrm_hdr *h = (void *)(in + 8), *rh = (void *)(out + 8);
	uint8_t *payload = PAYLOAD(in), *rpayload = PAYLOAD(out);
	uint32_t rlen = 0;
	int fd, st = 0;

	memcpy(out, in, 8);	/* the call's sequence number */
	if (inlen < 8 + sizeof(*h) || h->magic != XNUDRM_MAGIC || h->payload_len > inlen - 8 - sizeof(*h)) {
		*rh = (struct xnudrm_hdr){ .magic = XNUDRM_MAGIC, .status = -EINVAL };
		*outlen = 8 + sizeof(*rh);
		return;
	}
	*rh = *h;
	rh->payload_len = 0;
#ifdef XNUDRM_VIRGL
	if (virgl) {
		vg_serve(h, payload, rh, rpayload, &rlen);
		st = rh->status;
		goto out;
	}
#endif
	fd = client_fd(h->client);
	switch (h->op) {
	case XNUDRM_OPEN: {
		int c, f;
		if (h->arg0 != XNUDRM_VERSION) { st = -EPROTO; break; }
		for (c = 1; c < MAX_CLIENTS && clients[c]; c++) ;
		if (c == MAX_CLIENTS) { st = -EMFILE; break; }
		f = open(node, O_RDWR | O_CLOEXEC);
		if (f < 0) { st = -errno; break; }
		clients[c] = f + 1;
		rh->client = c;
		if (verbose) fprintf(stderr, "open: client %d\n", c);
		break;
	}
	case XNUDRM_CLOSE:
		if (fd < 0) { st = -EBADF; break; }
		drop_client(h->client);
		if (verbose) fprintf(stderr, "close: client %u\n", h->client);
		break;
	case XNUDRM_IOCTL:
		if (fd < 0) { st = -EBADF; break; }
		memcpy(rpayload, payload, h->payload_len);
		st = do_ioctl(fd, h->client, (uint32_t)h->arg0, rpayload, h->payload_len);
		rlen = h->payload_len;
		if (verbose > 1) fprintf(stderr, "ioctl %#x: %d\n", (unsigned)h->arg0, st);
		break;
	case XNUDRM_MAP:
		if (fd < 0) { st = -EBADF; break; }
		st = do_map(fd, h->client, rh, rpayload, &rlen);
		break;
	case XNUDRM_UNMAP:
		if (h->arg0 == 0 || h->arg0 >= MAX_MAPS || !maps[h->arg0].va || maps[h->arg0].client != (int)h->client) { st = -EINVAL; break; }
		munmap(maps[h->arg0].va, maps[h->arg0].len);
		maps[h->arg0].va = NULL;
		break;
	case XNUDRM_FENCE_WAIT:
	case XNUDRM_FENCE_CLOSE: {
		struct fence *f = h->arg0 && h->arg0 < MAX_FENCES ? &fences[h->arg0] : NULL;
		if (!f || !f->fd || f->client != (int)h->client) { st = -EINVAL; break; }
		if (h->op == XNUDRM_FENCE_CLOSE) { close(f->fd - 1); f->fd = 0; break; }
		struct pollfd p = { .fd = f->fd - 1, .events = POLLIN };
		int ms = h->arg1 ? (int)((h->arg1 + 999999) / 1000000) : 0;
		int r = poll(&p, 1, ms > 50 ? 50 : ms);	/* never hold the hosted CPU in a host call longer than 50 ms */
		st = r < 0 ? -errno : r == 0 ? -XNUDRM_ETIME : 0;
		break;
	}
	default:
		st = -ENOSYS;
	}
	rh->status = st;
#ifdef XNUDRM_VIRGL
out:
#endif
	rh->payload_len = st == 0 || h->op == XNUDRM_IOCTL ? rlen : 0;
	*outlen = 8 + sizeof(*rh) + rh->payload_len;
}

int main(int argc, char **argv)
{
	int a;
	for (a = 1; a < argc; a++) {
		if (!strcmp(argv[a], "-v")) verbose++;
		else if (!strcmp(argv[a], "--virgl")) virgl = 1;
		else node = argv[a];
	}
	if (!virgl) {	/* a render node that is not virtio-gpu: be the virtio-gpu device */
		char name[32] = "";
		struct drm_version v = { .name = name, .name_len = sizeof(name) - 1 };
		int f = open(node, O_RDWR | O_CLOEXEC);
		if (f >= 0 && !ioctl(f, DRM_IOCTL_VERSION, &v) && strcmp(name, "virtio_gpu")) virgl = 1;
		if (f >= 0) close(f);
	}
#ifdef XNUDRM_VIRGL
	if (virgl && vg_init()) return 1;
#else
	if (virgl) { fprintf(stderr, "xnudrmd: %s is not virtio-gpu, and this build has no virglrenderer backend\n", node); return 1; }
#endif
	hc = open("/dev/xnu-hostcall", O_RDWR | O_CLOEXEC);
	if (hc < 0) { perror("/dev/xnu-hostcall"); return 1; }
	msg = malloc(8 + XNUDRM_MAX_MSG + sizeof(struct xnudrm_hdr));
	rep = malloc(8 + XNUDRM_MAX_MSG + sizeof(struct xnudrm_hdr));
	if (!msg || !rep) return 1;
	fprintf(stderr, "xnudrmd: serving %s\n", virgl ? "virglrenderer (Venus)" : node);
	for (;;) {
		ssize_t n = read(hc, msg, 8 + XNUDRM_MAX_MSG + sizeof(struct xnudrm_hdr));
		size_t rn;
		if (n < 0) {
			if (errno == EINTR || errno == EAGAIN) continue;
			perror("read");
			return 1;
		}
		struct timespec t0, t1;
		if (verbose) clock_gettime(CLOCK_MONOTONIC, &t0);
		serve(msg, (size_t)n, rep, &rn);
		if (verbose) {	/* the hosted CPU that made the call waits this long */
			const struct xnudrm_hdr *h = (const void *)(msg + 8);
			long ms;
			clock_gettime(CLOCK_MONOTONIC, &t1);
			ms = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
			if (ms >= 20 && h->op != XNUDRM_FENCE_WAIT) fprintf(stderr, "slow: op %u arg0 %#llx of client %u took %ld ms\n", h->op, (unsigned long long)h->arg0, h->client, ms);
		}
		if (write(hc, rep, rn) < 0 && errno != ESTALE) perror("write");
	}
}

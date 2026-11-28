// SPDX-License-Identifier: MIT
/*
 * libxnudrm: Linux's virtio-gpu render node for Mesa's Venus driver inside the hosted macOS. The node is a user client of
 * kexts/XnuDrm; each DRM ioctl becomes one message to xnudrmd on the Linux host (xnudrm_proto.h), with the ioctl struct's
 * user pointers turned into offsets into the message. Buffer mappings are Linux's own pages, mapped into this process by
 * the kext. Fences are pipes whose read end stands for the fence (so dup'ed descriptors stay the same fence); poll() on
 * one asks xnudrmd, and once the fence signals the pipe is made readable for good.
 */
#ifdef XNUDRM_COMPAT_H	/* force-included into the whole driver: this file means the system's own calls */
#undef open
#undef close
#undef mmap
#undef munmap
#undef stat
#undef poll
#undef os_dupfd_cloexec
#endif
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <IOKit/IOKitLib.h>
#include "drm-uapi/drm.h"
#include "drm-uapi/virtgpu_drm.h"
#include "xnudrm_proto.h"
#include "xf86drm.h"

#define NODE_RENDER "/dev/dri/renderD128"
#define NODE_PRIMARY "/dev/dri/card0"

static pthread_mutex_t lk = PTHREAD_MUTEX_INITIALIZER;
static io_connect_t conn;		/* the user client of the call in progress (under lk) */
static struct xnode { int fd; io_connect_t conn; } nodes[64];	/* open render nodes: a /dev/null fd each */
static unsigned nnodes;
static struct xmap { void *va; size_t len; uint64_t id; io_connect_t conn; } *maps;
static unsigned nmaps;
static struct xfence { dev_t dev; ino_t ino; int wfd; uint64_t id; io_connect_t conn; unsigned refs; } *fences;	/* refs: descriptors of the read end */
static unsigned nfences;
struct xsfence { uint64_t id; io_connect_t conn; unsigned refs; };
static struct xsfd { int fd; struct xsfence *f; } *sfds;	/* sandbox fences (when SYS_pipe is denied) */
static unsigned nsfds;
static uint8_t *buf;			/* message, then reply */

extern int os_dupfd_cloexec(int fd);
static struct xfence *fence_of(int fd);

/* Open a harmless placeholder fd for a render node or sandbox fence.
 * Strict sandboxes (e.g. com.apple.WebKit.GPUProcess.sb) deny open("/dev/null")
 * with EPERM while allowing read-only opens of /private/etc/passwd. */
static int open_dummy_fd(void)	/* under lk */
{
	static int denied;	/* a sandbox is for good: ask (and be logged) once */
	if (!denied) {
		int fd = open("/dev/null", O_RDWR | O_CLOEXEC);
		if (fd >= 0 || errno != EPERM) return fd;
		denied = 1;
	}
	return open("/etc/passwd", O_RDONLY | O_CLOEXEC);
}

/* A new descriptor has this number: an entry still there is of one closed behind our back (an exported fence fd, closed by
 * the application itself), and would answer for the new one. No host call here (buf may hold a reply being copied out):
 * an unsignalled fence stays with xnudrmd until the node closes. Under lk. */
static void forget_sfd(int fd)
{
	for (unsigned i = 0; i < nsfds; i++) {
		if (sfds[i].fd == fd) {
			struct xsfence *sf = sfds[i].f;
			sfds[i] = sfds[--nsfds];
			if (--sf->refs == 0) free(sf);
			return;
		}
	}
}

static int debug(void) { static int d = -1; if (d < 0) d = getenv("XNUDRM_DEBUG") != NULL; return d; }

static int open_conn(io_connect_t *c)
{
	/* the user client hangs off the Metal accelerator (an IOAccelerator: sandboxes let drawing processes open it) */
	io_service_t sv = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("XnuCpuMetalAccelerator"));
	if (!sv) sv = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("XnuDrm"));
	if (!sv) { if (debug()) fprintf(stderr, "xnudrm: no XnuDrm service\n"); return -ENODEV; }
	kern_return_t kr = IOServiceOpen(sv, mach_task_self(), XNUDRM_CONNECT_TYPE, c);
	IOObjectRelease(sv);
	if (kr != KERN_SUCCESS) { if (debug()) fprintf(stderr, "xnudrm: IOServiceOpen %#x\n", kr); return -EIO; }
	if (!buf) buf = malloc(sizeof(struct xnudrm_hdr) + XNUDRM_MAX_MSG);
	if (!buf) { IOServiceClose(*c); return -ENOMEM; }
	return 0;
}

/* The user client behind a render node descriptor (0: not one). Called with lk held. */
static io_connect_t node_conn(int fd)
{
	for (unsigned i = 0; i < nnodes; i++) if (nodes[i].fd == fd) return nodes[i].conn;
	return 0;
}

/* buf holds a header and plen bytes of payload; the reply replaces it. Returns the reply's status or -errno. */
static int call(uint32_t plen)
{
	uint64_t in[4] = { (uintptr_t)buf, sizeof(struct xnudrm_hdr) + plen, (uintptr_t)buf, sizeof(struct xnudrm_hdr) + XNUDRM_MAX_MSG };
	uint64_t out = 0;
	uint32_t nout = 1;
	kern_return_t kr = IOConnectCallScalarMethod(conn, 0, in, 4, &out, &nout);
	if (kr != KERN_SUCCESS) { if (debug()) fprintf(stderr, "xnudrm: call %#x\n", kr); return -EIO; }
	return ((struct xnudrm_hdr *)buf)->status;
}

static struct xnudrm_hdr *hdr(uint32_t op, uint64_t a0, uint64_t a1)
{
	struct xnudrm_hdr *h = (void *)buf;
	*h = (struct xnudrm_hdr){ .magic = XNUDRM_MAGIC, .op = op, .arg0 = a0, .arg1 = a1 };
	return h;
}

/* ---- the render node as a file ---- */

int xnudrm_open(const char *path, int flags, ...)
{
	if (path && !strcmp(path, NODE_RENDER)) {
		io_connect_t c = 0;
		pthread_mutex_lock(&lk);
		int r = nnodes < sizeof(nodes) / sizeof(nodes[0]) ? open_conn(&c) : -EMFILE;
		int fd = r ? -1 : open_dummy_fd();
		if (!r && fd < 0) { IOServiceClose(c); r = -errno; }
		if (!r) nodes[nnodes++] = (struct xnode){ fd, c };
		pthread_mutex_unlock(&lk);
		if (r) { errno = -r; return -1; }
		return fd;
	}
	if (flags & O_CREAT) {
		va_list ap; va_start(ap, flags); int mode = va_arg(ap, int); va_end(ap);
		return open(path, flags, mode);
	}
	return open(path, flags);
}

int xnudrm_close(int fd)
{
	pthread_mutex_lock(&lk);
	for (unsigned i = 0; i < nnodes; i++) {
		if (nodes[i].fd == fd) {
			/* closing the user client drops what is still mapped from it and its DRM file on Linux */
			io_connect_t c = nodes[i].conn;
			nodes[i] = nodes[--nnodes];
			for (unsigned j = 0; j < nmaps; ) { if (maps[j].conn == c) maps[j] = maps[--nmaps]; else j++; }
			for (unsigned j = 0; j < nfences; j++) {
				if (fences[j].conn == c && fences[j].id) {
					fences[j].id = 0;
					/* end of file makes the read end readable; no write: without a reader left (a fence
					 * nobody waited for) it would raise SIGPIPE */
					if (fences[j].wfd >= 0) close(fences[j].wfd);
					fences[j].wfd = -1;
				}
			}
			for (unsigned j = 0; j < nsfds; j++)
				if (sfds[j].f->conn == c) sfds[j].f->id = 0;
			IOServiceClose(c);
			break;
		}
	}
	for (unsigned i = 0; i < nsfds; i++) {
		if (sfds[i].fd == fd) {
			struct xsfence *sf = sfds[i].f;
			sfds[i] = sfds[--nsfds];
			if (--sf->refs == 0) {
				if (sf->id) {
					conn = sf->conn;
					hdr(XNUDRM_FENCE_CLOSE, sf->id, 0);
					call(0);
				}
				free(sf);
			}
			break;
		}
	}
	struct xfence *f = nfences ? fence_of(fd) : NULL;
	if (f && --f->refs == 0) {	/* the read end's last descriptor: nobody can wait for this fence any more */
		if (f->wfd >= 0) close(f->wfd);
		if (f->id) { conn = f->conn; hdr(XNUDRM_FENCE_CLOSE, f->id, 0); call(0); }
		*f = fences[--nfences];
	}
	pthread_mutex_unlock(&lk);
	return close(fd);
}

int xnudrm_dupfd_cloexec(int fd)
{
	static int denied;
	pthread_mutex_lock(&lk);
	/* Strict sandboxes (e.g. com.apple.WebKit.GPUProcess.sb) deny fcntl(F_DUPFD_CLOEXEC)
	 * with EPERM while allowing SYS_dup (and F_SETFD). */
	int nfd = denied ? -1 : os_dupfd_cloexec(fd);
	if (denied || (nfd < 0 && errno == EPERM)) {
		int low[3], nlow = 0;
		denied = 1;
		while ((nfd = dup(fd)) >= 0 && nfd < 3) low[nlow++] = nfd;	/* never stdin, stdout or stderr */
		int e = errno;
		while (nlow) close(low[--nlow]);
		if (nfd >= 0 && fcntl(nfd, F_SETFD, FD_CLOEXEC)) { e = errno; close(nfd); nfd = -1; }
		errno = e;
	}
	if (nfd < 0) { pthread_mutex_unlock(&lk); return -1; }
	forget_sfd(nfd);
	struct xfence *f = nfences ? fence_of(fd) : NULL;
	if (f) f->refs++;
	for (unsigned i = 0; i < nsfds; i++) {
		if (sfds[i].fd == fd) {
			struct xsfence *sf = sfds[i].f;
			struct xsfd *ns = realloc(sfds, (nsfds + 1) * sizeof(*sfds));
			if (!ns) {
				pthread_mutex_unlock(&lk);
				close(nfd);
				errno = ENOMEM;
				return -1;
			}
			sfds = ns;
			sf->refs++;
			sfds[nsfds++] = (struct xsfd){ nfd, sf };
			break;
		}
	}
	pthread_mutex_unlock(&lk);
	return nfd;
}

int xnudrm_stat(const char *path, struct stat *st)
{
	if (path && !strcmp(path, NODE_RENDER)) {
		memset(st, 0, sizeof(*st));
		st->st_mode = S_IFCHR | 0666;
		st->st_rdev = makedev(226, 128);
		return 0;
	}
	if (path && !strcmp(path, NODE_PRIMARY)) { errno = ENOENT; return -1; }
	return stat(path, st);
}

/* ---- ioctls ---- */

/* Append len bytes at p (or zeros) to the payload; the field becomes the offset. */
static int put(uint32_t *plen, uint64_t *field, const void *p, uint64_t len)
{
	if (!len) { *field = 0; return 0; }
	if (*plen + len > XNUDRM_MAX_MSG) return -E2BIG;
	uint8_t *pl = buf + sizeof(struct xnudrm_hdr);
	if (p) memcpy(pl + *plen, p, len); else memset(pl + *plen, 0, len);
	*field = *plen;
	*plen += (uint32_t)((len + 7) & ~7ull);
	return 0;
}

static int new_fence(uint64_t id)	/* for conn */
{
	static int denied;
	int p[2];
	if (!denied && pipe(p) == 0) {
		fcntl(p[0], F_SETFD, FD_CLOEXEC); fcntl(p[1], F_SETFD, FD_CLOEXEC);
		struct stat st; fstat(p[0], &st);
		/* a pipe's inode number is reused once every descriptor of an earlier pipe is closed: an entry for that dead pipe
		 * would answer for this one */
		for (unsigned i = 0; i < nfences; ) {
			if (fences[i].ino == st.st_ino && fences[i].dev == st.st_dev) {
				/* (an unsignalled one's fence stays with xnudrmd until the node closes: no host call here, buf holds the
				 * reply being copied out) */
				if (fences[i].wfd >= 0) close(fences[i].wfd);
				fences[i] = fences[--nfences];
			} else i++;
		}
		struct xfence *nf = realloc(fences, (nfences + 1) * sizeof(*fences));
		if (!nf) { close(p[0]); close(p[1]); errno = ENOMEM; return -1; }
		fences = nf;
		fences[nfences++] = (struct xfence){ st.st_dev, st.st_ino, p[1], id, conn, 1 };
		return p[0];
	}
	if (!denied && errno != EPERM) return -1;
	denied = 1;
	/* Strict sandboxes (e.g. com.apple.WebKit.GPUProcess.sb) deny SYS_pipe:
	 * use a dummy fd and track fence identity and readiness in userspace. */
	int fd = open_dummy_fd();
	if (fd < 0) return -1;
	struct xsfence *sf = malloc(sizeof(*sf));
	struct xsfd *ns = sf ? realloc(sfds, (nsfds + 1) * sizeof(*sfds)) : NULL;
	if (!ns) { free(sf); close(fd); errno = ENOMEM; return -1; }
	sfds = ns;
	forget_sfd(fd);
	*sf = (struct xsfence){ id, conn, 1 };
	sfds[nsfds++] = (struct xsfd){ fd, sf };
	return fd;
}

static int do_ioctl(unsigned long request, void *arg)
{
	uint32_t nr = request & 0xff, size = (request >> 16) & 0x1fff, plen = 0;
	uint8_t *pl = buf + sizeof(struct xnudrm_hdr);
	uint64_t f0 = 0, f1 = 0, lost = 0;
	int r;

	if (size > 256) return -EINVAL;
	if (nr == DRM_COMMAND_BASE + DRM_VIRTGPU_MAP) {	/* answered here: see xnudrm_mmap */
		struct drm_virtgpu_map *m = arg;
		m->offset = (uint64_t)m->handle << 12;
		return 0;
	}
	hdr(XNUDRM_IOCTL, nr, 0);
	memcpy(pl, arg, size);
	plen = (size + 7) & ~7u;
	switch (nr) {
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GETPARAM: {
		struct drm_virtgpu_getparam *a = (void *)pl;
		f0 = a->value; if ((r = put(&plen, &a->value, NULL, 8))) return r;
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GET_CAPS: {
		struct drm_virtgpu_get_caps *a = (void *)pl;
		f0 = a->addr; if ((r = put(&plen, &a->addr, NULL, a->size))) return r;
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_CONTEXT_INIT: {
		struct drm_virtgpu_context_init *a = (void *)pl;
		if ((r = put(&plen, &a->ctx_set_params, (void *)(uintptr_t)a->ctx_set_params, (uint64_t)a->num_params * sizeof(struct drm_virtgpu_context_set_param)))) return r;
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_CREATE_BLOB: {
		struct drm_virtgpu_resource_create_blob *a = (void *)pl;
		if ((r = put(&plen, &a->cmd, (void *)(uintptr_t)a->cmd, a->cmd_size))) return r;
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_EXECBUFFER: {
		struct drm_virtgpu_execbuffer *a = (void *)pl;
		if (a->num_in_syncobjs || a->num_out_syncobjs || (a->flags & VIRTGPU_EXECBUF_FENCE_FD_IN)) return -EOPNOTSUPP;
		if ((r = put(&plen, &a->command, (void *)(uintptr_t)a->command, a->size))) return r;
		if ((r = put(&plen, &a->bo_handles, (void *)(uintptr_t)a->bo_handles, (uint64_t)a->num_bo_handles * 4))) return r;
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_INFO:
	case DRM_COMMAND_BASE + DRM_VIRTGPU_WAIT:
	case 0x09:	/* GEM_CLOSE */
		break;
	default:
		return -ENOTTY;	/* PRIME, syncobj: not over the bridge */
	}
	r = call(plen);
	if (r && r != -EINTR) { if (debug()) fprintf(stderr, "xnudrm: ioctl %#x: %d\n", nr, r); }
	/* copy the outputs back: the struct, then the out-buffers its pointers named */
	switch (nr) {
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GETPARAM: {
		struct drm_virtgpu_getparam *a = (void *)pl;
		if (!r) memcpy((void *)(uintptr_t)f0, pl + a->value, sizeof(int));
		a->value = f0;
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_GET_CAPS: {
		struct drm_virtgpu_get_caps *a = (void *)pl;
		if (!r) memcpy((void *)(uintptr_t)f0, pl + a->addr, a->size);
		a->addr = f0;
		break;
	}
	case DRM_COMMAND_BASE + DRM_VIRTGPU_CONTEXT_INIT:
	case DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_CREATE_BLOB:
	case DRM_COMMAND_BASE + DRM_VIRTGPU_EXECBUFFER: {
		/* pointer fields go back to the caller's values */
		uint8_t saved[256];
		memcpy(saved, arg, size);
		if (nr == DRM_COMMAND_BASE + DRM_VIRTGPU_CONTEXT_INIT) ((struct drm_virtgpu_context_init *)pl)->ctx_set_params = ((struct drm_virtgpu_context_init *)saved)->ctx_set_params;
		if (nr == DRM_COMMAND_BASE + DRM_VIRTGPU_RESOURCE_CREATE_BLOB) ((struct drm_virtgpu_resource_create_blob *)pl)->cmd = ((struct drm_virtgpu_resource_create_blob *)saved)->cmd;
		if (nr == DRM_COMMAND_BASE + DRM_VIRTGPU_EXECBUFFER) {
			struct drm_virtgpu_execbuffer *a = (void *)pl, *s = (void *)saved;
			a->command = s->command; a->bo_handles = s->bo_handles;
			if (!r && (a->flags & VIRTGPU_EXECBUF_FENCE_FD_OUT)) {
				uint32_t id = (uint32_t)a->fence_fd;
				errno = EMFILE;
				if ((a->fence_fd = new_fence(id)) < 0) { r = -errno; lost = id; }
			}
			if (debug()) fprintf(stderr, "xnudrm: execbuffer flags %#x size %u ring %u -> %d, fence fd %d\n", a->flags, a->size, a->ring_idx, r, a->fence_fd);
		}
		break;
	}
	}
	(void)f1;
	memcpy(arg, pl, size);
	if (lost) { hdr(XNUDRM_FENCE_CLOSE, lost, 0); call(0); }	/* no descriptor for it here (buf is free now) */
	return r;
}

int drmIoctl(int fd, unsigned long request, void *arg)
{
	/* A blocking VIRTGPU_WAIT would hold the calling XNU CPU inside fki_hostcall (deaf to IPIs):
	 * poll with VIRTGPU_WAIT_NOWAIT and sleep in macOS userspace with exponential backoff up to 2 ms. */
	if ((request & 0xff) == DRM_COMMAND_BASE + DRM_VIRTGPU_WAIT && arg &&
	    !(((struct drm_virtgpu_3d_wait *)arg)->flags & VIRTGPU_WAIT_NOWAIT)) {
		long delay_ns = 100000;	/* 100 us -> 2 ms */
		for (;;) {
			struct drm_virtgpu_3d_wait nw = *(struct drm_virtgpu_3d_wait *)arg;
			nw.flags |= VIRTGPU_WAIT_NOWAIT;
			pthread_mutex_lock(&lk);
			conn = node_conn(fd);
			int r = conn ? do_ioctl(request, &nw) : -EBADF;
			pthread_mutex_unlock(&lk);
			if (r != -EBUSY) {
				if (r) { errno = -r; return -1; }
				return 0;
			}
			struct timespec d = { 0, delay_ns };
			nanosleep(&d, NULL);
			if (delay_ns < 2000000) {
				delay_ns <<= 1;
				if (delay_ns > 2000000) delay_ns = 2000000;
			}
		}
	}
	pthread_mutex_lock(&lk);
	conn = node_conn(fd);
	int r = conn ? do_ioctl(request, arg) : -EBADF;
	pthread_mutex_unlock(&lk);
	if (r) { errno = -r; return -1; }
	return 0;
}

/* ---- mappings: mmap(node fd, offset) after DRM_IOCTL_VIRTGPU_MAP ---- */

/*
 * VIRTGPU_MAP's offset is the GEM handle's mmap cookie on Linux; xnudrmd maps by handle, so the shim answers MAP itself
 * with the handle as the offset (MAP is never forwarded) and mmap() maps that handle.
 */
void *xnudrm_mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off)
{
	pthread_mutex_lock(&lk);
	io_connect_t c = fd >= 0 ? node_conn(fd) : 0;
	if (!c) { pthread_mutex_unlock(&lk); return mmap(addr, len, prot, flags, fd, off); }
	uint64_t in[2] = { (uint64_t)off >> 12, len }, out[2] = { 0, 0 };
	uint32_t nout = 2;
	kern_return_t kr = IOConnectCallScalarMethod(c, 1, in, 2, out, &nout);
	if (kr == KERN_SUCCESS) {
		struct xmap *nm = realloc(maps, (nmaps + 1) * sizeof(*maps));
		if (nm) { maps = nm; maps[nmaps++] = (struct xmap){ (void *)(uintptr_t)out[0], len, out[1], c }; }
	}
	pthread_mutex_unlock(&lk);
	if (kr != KERN_SUCCESS) { if (debug()) fprintf(stderr, "xnudrm: map handle %llu: %#x\n", (unsigned long long)in[0], kr); errno = ENOMEM; return MAP_FAILED; }
	if (addr && (flags & MAP_FIXED) && addr != (void *)(uintptr_t)out[0]) {
		/* a placed mapping (VkPhysicalDeviceMapMemoryPlacedFeaturesEXT): not supported over the bridge */
		xnudrm_munmap((void *)(uintptr_t)out[0], len);
		errno = EINVAL;
		return MAP_FAILED;
	}
	return (void *)(uintptr_t)out[0];
}

int xnudrm_munmap(void *addr, size_t len)
{
	pthread_mutex_lock(&lk);
	for (unsigned i = 0; i < nmaps; i++) {
		if (maps[i].va == addr) {
			uint64_t id = maps[i].id;
			io_connect_t c = maps[i].conn;
			maps[i] = maps[--nmaps];
			IOConnectCallScalarMethod(c, 2, &id, 1, NULL, NULL);
			pthread_mutex_unlock(&lk);
			return 0;
		}
	}
	pthread_mutex_unlock(&lk);
	return munmap(addr, len);
}

/* ---- fences ---- */

static struct xfence *fence_of(int fd)
{
	struct stat st;
	if (fstat(fd, &st)) return NULL;
	for (unsigned i = 0; i < nfences; i++)
		if (fences[i].ino == st.st_ino && fences[i].dev == st.st_dev) return &fences[i];
	return NULL;
}

static struct xsfence *sfd_of(int fd)
{
	for (unsigned i = 0; i < nsfds; i++)
		if (sfds[i].fd == fd) return sfds[i].f;
	return NULL;
}

/* 1 signalled, 0 not yet, <0 error (not known to be signalled: the fence is kept). Called with lk held. */
static int fence_wait_id(uint64_t *id, io_connect_t c)
{
	if (!*id) return 1;
	conn = c;
	hdr(XNUDRM_FENCE_WAIT, *id, 0);
	int r = call(0);
	if (r == -XNUDRM_ETIME || r == -EINTR) return 0;
	if (r) {
		if (debug()) fprintf(stderr, "xnudrm: fence %llu wait: %d\n", (unsigned long long)*id, r);
		return -EIO;
	}
	hdr(XNUDRM_FENCE_CLOSE, *id, 0);
	call(0);
	*id = 0;
	return 1;
}

static int fence_check(struct xfence *f)
{
	int r = fence_wait_id(&f->id, f->conn);
	if (r > 0 && f->wfd >= 0) {
		(void)!write(f->wfd, "s", 1);	/* signalled for good */
		close(f->wfd);
		f->wfd = -1;
	}
	return r;
}

int xnudrm_poll(struct pollfd *fds, nfds_t n, int timeout)
{
	struct timespec t0, now;
	struct pollfd rest[n + 1];		/* fds without the sandbox fences */
	long delay_ns = 100000;	/* 100 us -> 2 ms */
	clock_gettime(CLOCK_MONOTONIC, &t0);
	for (;;) {
		int pending = 0, ready = 0, err = 0, n_sf = 0, n_rest = 0;
		pthread_mutex_lock(&lk);
		for (nfds_t i = 0; i < n; i++) {
			rest[i] = fds[i];
			fds[i].revents = 0;
			if (fds[i].fd < 0) continue;
			struct xsfence *sf = nsfds ? sfd_of(fds[i].fd) : NULL;
			int r;
			if (sf) {
				n_sf++;
				rest[i].fd = -1;
				r = fence_wait_id(&sf->id, sf->conn);
				if (r > 0 && (fds[i].revents = fds[i].events & (POLLIN | POLLRDNORM))) ready++;
			} else {
				struct xfence *f = nfences ? fence_of(fds[i].fd) : NULL;
				n_rest++;
				r = f && f->id ? fence_check(f) : 1;
			}
			if (r == 0) pending = 1;
			if (r < 0) err = -r;
		}
		if (debug()) {
			fprintf(stderr, "xnudrm: poll %u fds, timeout %d: %s;", (unsigned)n, timeout, err ? "error" : pending ? "pending" : "ready");
			for (nfds_t i = 0; i < n; i++) {
				struct xsfence *sf = nsfds ? sfd_of(fds[i].fd) : NULL;
				struct xfence *f = (!sf && nfences) ? fence_of(fds[i].fd) : NULL;
				uint64_t id = sf ? sf->id : (f ? f->id : 0);
				fprintf(stderr, " fd %d -> %s id %llu", fds[i].fd, (sf || f) ? "fence" : "other", (unsigned long long)id);
			}
			fprintf(stderr, "\n");
		}
		pthread_mutex_unlock(&lk);
		if (err) { errno = err; return -1; }
		if (!n_sf && !pending) return poll(fds, n, timeout);	/* signalled pipe fences are readable now */
		if (n_rest) {	/* not for sandbox fences: where there are any, SYS_poll is denied */
			int r = poll(rest, n, 0);
			if (r < 0 && !ready) return -1;
			for (nfds_t i = 0; r > 0 && i < n; i++) if (rest[i].fd >= 0) fds[i].revents = rest[i].revents;
			if (r > 0) ready += r;
		}
		if (ready) return ready;
		clock_gettime(CLOCK_MONOTONIC, &now);
		long ms = (now.tv_sec - t0.tv_sec) * 1000 + (now.tv_nsec - t0.tv_nsec) / 1000000;
		if (timeout >= 0 && ms >= timeout) return 0;
		/* each check stops the hosted CPU for one host call: back off exponentially up to 2 ms */
		struct timespec d = { 0, delay_ns };
		nanosleep(&d, NULL);
		if (delay_ns < 2000000) {
			delay_ns <<= 1;
			if (delay_ns > 2000000) delay_ns = 2000000;
		}
	}
}

/* ---- device discovery ---- */

int drmGetDevices2(uint32_t flags, drmDevicePtr devices[], int max)
{
	if (!devices) return 1;
	if (max < 1) return 0;
	drmDevicePtr d = calloc(1, sizeof(*d) + DRM_NODE_MAX * sizeof(char *) + sizeof(drmPciBusInfo) + sizeof(drmPciDeviceInfo));
	if (!d) return -ENOMEM;
	d->nodes = (char **)(d + 1);
	d->nodes[DRM_NODE_PRIMARY] = NODE_PRIMARY;
	d->nodes[DRM_NODE_RENDER] = NODE_RENDER;
	d->available_nodes = 1 << DRM_NODE_RENDER;
	d->bustype = DRM_BUS_PCI;
	d->businfo.pci = (drmPciBusInfoPtr)(d->nodes + DRM_NODE_MAX);
	d->deviceinfo.pci = (drmPciDeviceInfoPtr)(d->businfo.pci + 1);
	d->deviceinfo.pci->vendor_id = 0x1af4;	/* virtio */
	d->deviceinfo.pci->device_id = 0x1050;	/* virtio-gpu */
	devices[0] = d;
	return 1;
}

void drmFreeDevices(drmDevicePtr devices[], int count)
{
	for (int i = 0; i < count; i++) free(devices[i]);
}

drmVersionPtr drmGetVersion(int fd)
{
	pthread_mutex_lock(&lk);
	io_connect_t c = node_conn(fd);
	pthread_mutex_unlock(&lk);
	if (!c) return NULL;
	drmVersionPtr v = calloc(1, sizeof(*v));
	if (!v) return NULL;
	v->name = "virtio_gpu";
	v->name_len = 10;
	v->desc = "virtio GPU over the xnulinux DRM bridge";
	v->date = "0";
	return v;
}

void drmFreeVersion(drmVersionPtr v) { free(v); }

// SPDX-License-Identifier: GPL-2.0-only
/*
 * xnudrmd with its virglrenderer backend, asked what the hosted macOS asks it: the requests of Mesa's Venus driver
 * (src/virtio/vulkan/vn_renderer_virtgpu.c, as libxnudrm and kexts/XnuDrm pass them on), each through xnudrmd.c's
 * serve() the way the service loop hands it over, answered by libvirglrenderer and its render server on this machine's
 * own Vulkan drivers. The Venus commands are real ones: a ring, an instance, every device and its memory.
 *
 * No fki and no hosted system: /dev/xnu-hostcall's ioctls are stood in for by hc_ioctl(), which keeps
 * fki-ko/fki_hostcall.c's rules, writes down what xnudrmd asks to have pinned and translated, and gives every page a
 * made-up physical address that the test turns back into its own address: it reads and writes a mapping the way the
 * hosted kernel does, through the ranges it was given.
 *
 * Run: make test-virgl. Each case runs in a process of its own, with a renderer of its own. Where virglrenderer gives
 * no Venus context the test says so and passes. The cases that need a Vulkan device use the renderer's software
 * devices (llvmpipe) and say so when there is none. A hardware GPU the renderer also lists is left alone: these are
 * hand-encoded commands, and a machine's own GPU is not what to try them on.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <drm/drm.h>
#include <drm/virtgpu_drm.h>
#include <virglrenderer.h>

/* xnudrmd.c itself, its ioctls on /dev/xnu-hostcall coming here (its headers are above: they are not to see the names) */
static int hc_ioctl(int fd, unsigned long req, void *arg);
#define main xnudrmd_main
#define ioctl hc_ioctl
#include "../xnudrmd.c"
#undef main
#undef ioctl

static int bad;	/* failed checks of this case */
static void fail(int line, const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	printf("  virgl.c:%d: ", line); vprintf(fmt, ap); printf("\n");
	va_end(ap);
	bad++;
}
#define CHECK(x) do { if (!(x)) fail(__LINE__, "%s", #x); } while (0)
#define EQ(x, want) do { long long x_ = (x), w_ = (want); if (x_ != w_) fail(__LINE__, "%s is %lld, not %lld", #x, x_, w_); } while (0)
#define MUST(x) do { if (!(x)) { fail(__LINE__, "%s", #x); printf("  (the case stops here)\n"); fflush(stdout); _exit(1); } } while (0)
static void say(const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	printf("  "); vprintf(fmt, ap); printf("\n");
	va_end(ap);
}

/* ---- /dev/xnu-hostcall's ioctls, by fki-ko/fki_hostcall.c's rules ---- */

#define HC 1000000			/* xnudrmd's descriptor of the device: no real one */
#define PA0 (1ull << 52)
#define PG(x) (((uint64_t)(x) + 4095) & ~4095ull)
static struct { uint64_t va, len; } pins[1024];
static unsigned npins, n_pin, n_unpin, n_xlate, n_refused;	/* refused: what fki would not have done */
static struct xnu_hostcall_xlate last_x;
static struct xnu_hostcall_range last_pin;

/* Three pages in a row, then a hole: a mapping comes back as many ranges, and each address can be turned back. */
static uint64_t fake_pa(uint64_t va) { uint64_t vpn = va >> 12; return PA0 + ((vpn + vpn / 3) << 12); }
static uint64_t fake_va(uint64_t pa) { uint64_t p = (pa - PA0) >> 12; return pa < PA0 || (pa & 4095) || p % 4 == 3 ? 0 : (p - p / 4) << 12; }

/* The mappings a range lies in: 1 when one is a device's (VM_IO, VM_PFNMAP); name: what the first is a mapping of. */
static int vma(uint64_t va, uint64_t len, char *name, size_t nlen)
{
	FILE *f = fopen("/proc/self/smaps", "r");
	char line[512], path[256];
	unsigned long s, e;
	int in = 0, dev = 0;

	if (name) name[0] = 0;
	while (f && fgets(line, sizeof(line), f)) {
		path[0] = 0;
		if (sscanf(line, "%lx-%lx %*4s %*x %*x:%*x %*u %255[^\n]", &s, &e, path) >= 2) {
			in = s < va + len && e > va;
			if (in && s <= va && name) snprintf(name, nlen, "%s", path[0] ? path : "anonymous memory");
		} else if (in && !strncmp(line, "VmFlags:", 8) && (strstr(line, " io") || strstr(line, " pf"))) dev = 1;
	}
	if (f) fclose(f);
	return dev;
}

static int refused(int e) { n_refused++; errno = e; return -1; }

static int hc_ioctl(int fd, unsigned long req, void *arg)
{
	unsigned i;

	if (fd != HC) return ioctl(fd, req, arg);
	if (req == XNU_HOSTCALL_XLATE) {
		struct xnu_hostcall_xlate *x = arg;
		uint64_t n = x->len >> 12, *out = (uint64_t *)(uintptr_t)x->out;
		n_xlate++; last_x = *x;
		if ((x->va | x->len) & 4095) return refused(EINVAL);
		if (n > x->max) return refused(E2BIG);
		if (msync((void *)(uintptr_t)x->va, x->len, MS_ASYNC)) return refused(EFAULT);	/* not all of it is mapped */
		for (i = 0; i < n; i++) out[i] = fake_pa(x->va + ((uint64_t)i << 12));
		return 0;
	}
	if (req == XNU_HOSTCALL_PIN) {
		struct xnu_hostcall_range *r = arg;
		n_pin++; last_pin = *r;
		if (((r->va | r->len) & 4095) || !r->len || r->len > 1ull << 30 || r->va + r->len < r->va) return refused(EINVAL);
		if (vma(r->va, r->len, NULL, 0)) { errno = EOPNOTSUPP; return -1; }	/* nothing to pin: not a refusal */
		if (msync((void *)(uintptr_t)r->va, r->len, MS_ASYNC) || npins == sizeof(pins) / sizeof(pins[0])) return refused(EFAULT);
		pins[npins].va = r->va; pins[npins++].len = r->len;
		return 0;
	}
	if (req == XNU_HOSTCALL_UNPIN) {
		struct xnu_hostcall_range *r = arg;
		n_unpin++;
		for (i = 0; i < npins; i++) if (pins[i].va == r->va) { pins[i] = pins[--npins]; return 0; }
		return refused(ENOENT);
	}
	errno = ENOTTY;
	return -1;
}

static int pinned(uint64_t va, uint64_t len)
{
	for (unsigned i = 0; i < npins; i++) if (pins[i].va <= va && va + len <= pins[i].va + pins[i].len) return 1;
	return 0;
}

/* ---- the hosted side: the kext's host call, libxnudrm's payloads ---- */

static struct xnudrm_hdr *R;	/* the last reply, and its payload */
static uint8_t *RP;
static uint64_t call_seq = 0x100;

static int rpc(uint32_t op, uint32_t c, uint64_t a0, uint64_t a1, const void *payload, uint32_t plen)
{
	struct xnudrm_hdr *h = (void *)(msg + 8);
	size_t rn = 0;

	call_seq++;
	memcpy(msg, &call_seq, 8);
	*h = (struct xnudrm_hdr){ .magic = XNUDRM_MAGIC, .op = op, .client = c, .arg0 = a0, .arg1 = a1, .payload_len = plen };
	if (plen) memcpy(PAYLOAD(msg), payload, plen);
	memset(rep, 0xa5, 8 + sizeof(*h));
	serve(msg, 8 + sizeof(*h) + plen, rep, &rn);
	R = (void *)(rep + 8); RP = PAYLOAD(rep);
	/* what fki and the kext take a reply by: the call's number, the length, the magic */
	MUST(!memcmp(rep, msg, 8) && rn == 8 + sizeof(*R) + R->payload_len && R->magic == XNUDRM_MAGIC && R->op == op);
	return R->status;
}

static uint32_t xopen(void)
{
	MUST(rpc(XNUDRM_OPEN, 0, XNUDRM_VERSION, 0, NULL, 0) == 0 && R->client);
	return R->client;
}
static int xclose(uint32_t c) { return rpc(XNUDRM_CLOSE, c, 0, 0, NULL, 0); }

/* An ioctl's payload as libxnudrm lays it out: the struct, then what its pointers named, each rounded up to 8 bytes. */
static uint8_t pl[1 << 20];
static uint32_t plen;
static void *arg(const void *a, uint32_t size) { memcpy(pl, a, size); plen = (size + 7) & ~7u; return pl; }
static uint64_t put(const void *p, uint64_t len)
{
	uint64_t off = plen;
	if (!len) return 0;
	if (p) memcpy(pl + plen, p, len); else memset(pl + plen, 0, len);
	plen += (uint32_t)((len + 7) & ~7ull);
	return off;
}
static int ioc(uint32_t c, uint32_t nr)
{
	int r = rpc(XNUDRM_IOCTL, c, nr, 0, pl, plen);
	MUST(R->payload_len == (r == -EBADF ? 0 : plen));	/* the reply has the request's layout, also when the ioctl failed */
	return r;
}
#define VG(nr) (DRM_COMMAND_BASE + DRM_VIRTGPU_##nr)

static int getparam(uint32_t c, uint64_t param, uint64_t *val)
{
	struct drm_virtgpu_getparam *a = arg(&(struct drm_virtgpu_getparam){ .param = param }, sizeof(*a));
	uint64_t off = a->value = put(NULL, 8);
	int r = ioc(c, VG(GETPARAM));
	a = (void *)RP;
	*val = 0;	/* Venus zeroes it: the kernel writes an int */
	if (!R->payload_len) return r;
	CHECK(a->value == off && a->param == param);
	memcpy(val, RP + off, 8);
	return r;
}

static int get_caps(uint32_t c, uint32_t id, uint32_t ver, void *caps, uint32_t size)
{
	struct drm_virtgpu_get_caps *a = arg(&(struct drm_virtgpu_get_caps){ .cap_set_id = id, .cap_set_ver = ver, .size = size }, sizeof(*a));
	uint64_t off = a->addr = put(NULL, size);
	int r = ioc(c, VG(GET_CAPS));
	a = (void *)RP;
	CHECK(a->addr == off && a->size == size);
	memcpy(caps, RP + off, size);
	return r;
}

/* As Venus asks for its context: the capset, 64 rings, no events for fences. */
static int context_init(uint32_t c, int capset)
{
	struct drm_virtgpu_context_set_param sp[3] = {
		{ VIRTGPU_CONTEXT_PARAM_CAPSET_ID, capset }, { VIRTGPU_CONTEXT_PARAM_NUM_RINGS, 64 }, { VIRTGPU_CONTEXT_PARAM_POLL_RINGS_MASK, 0 },
	};
	int skip = capset < 0;
	struct drm_virtgpu_context_init *a = arg(&(struct drm_virtgpu_context_init){ .num_params = 3 - skip }, sizeof(*a));
	a->ctx_set_params = put(sp + skip, (3 - skip) * sizeof(sp[0]));
	return ioc(c, VG(CONTEXT_INIT));
}

static int blob_cmd(uint32_t c, uint32_t mem, uint32_t flags, uint64_t size, uint64_t blob_id, const void *cmd, uint32_t cmd_size, uint32_t *handle)
{
	struct drm_virtgpu_resource_create_blob *a = arg(&(struct drm_virtgpu_resource_create_blob){ .blob_mem = mem, .blob_flags = flags,
	    .size = size, .blob_id = blob_id, .cmd_size = cmd_size }, sizeof(*a));
	a->cmd = put(cmd, cmd_size);
	int r = ioc(c, VG(RESOURCE_CREATE_BLOB));
	a = (void *)RP;
	*handle = r ? 0 : a->bo_handle;
	if (!r) CHECK(a->bo_handle && a->res_handle && a->size == size);
	return r;
}
/* Venus's shared memory (rings, replies, uploads): host memory with blob id 0, mappable and nothing else. */
static uint32_t shm(uint32_t c, uint64_t size)
{
	uint32_t h;
	MUST(blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, size, 0, NULL, 0, &h) == 0);
	return h;
}

static int gem_close(uint32_t c, uint32_t handle)
{
	arg(&(struct drm_gem_close){ .handle = handle }, sizeof(struct drm_gem_close));
	return ioc(c, 0x09);
}

static int res_info(uint32_t c, uint32_t handle, struct drm_virtgpu_resource_info *info)
{
	arg(&(struct drm_virtgpu_resource_info){ .bo_handle = handle }, sizeof(*info));
	int r = ioc(c, VG(RESOURCE_INFO));
	memcpy(info, RP, sizeof(*info));
	return r;
}

/* Returns the status; *fence: the fence's id, 0 without one. */
static int execbuf(uint32_t c, const void *cmd, uint32_t size, uint32_t flags, uint32_t ring_idx, uint32_t *fence)
{
	struct drm_virtgpu_execbuffer *a = arg(&(struct drm_virtgpu_execbuffer){ .flags = flags, .size = size, .ring_idx = ring_idx,
	    .fence_fd = -1 }, sizeof(*a));
	a->command = put(cmd, size);
	int r = ioc(c, VG(EXECBUFFER));
	a = (void *)RP;
	if (fence) *fence = !r && (flags & VIRTGPU_EXECBUF_FENCE_FD_OUT) ? (uint32_t)a->fence_fd : 0;
	if (!r && !(flags & VIRTGPU_EXECBUF_FENCE_FD_OUT)) EQ(a->fence_fd, -1);
	return r;
}
/* An empty submit that leaves a fence, as virtgpu_submit does for a sync. */
static uint32_t fence_on(uint32_t c, uint32_t ring_idx)
{
	uint32_t f = 0;
	EQ(execbuf(c, NULL, 0, VIRTGPU_EXECBUF_RING_IDX | VIRTGPU_EXECBUF_FENCE_FD_OUT, ring_idx, &f), 0);
	CHECK(f > 0 && f < 65536);
	return f;
}
static int fence_wait(uint32_t c, uint32_t f, uint64_t ns) { return rpc(XNUDRM_FENCE_WAIT, c, f, ns, NULL, 0); }
static int fence_close(uint32_t c, uint32_t f) { return rpc(XNUDRM_FENCE_CLOSE, c, f, 0, NULL, 0); }
static double now_ms(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}
/* libxnudrm's poll(): ask without waiting, sleep in between. 0 when it signalled within ms. */
static int fence_poll(uint32_t c, uint32_t f, int ms)
{
	double end = now_ms() + ms;
	int r;
	while ((r = fence_wait(c, f, 0)) == -XNUDRM_ETIME && now_ms() < end) usleep(200);
	return r;
}

/*
 * A mapping as kexts/XnuDrm makes it (doMap): XNUDRM_MAP, the reply's ranges checked the way the kext checks them, then
 * the pages "mapped": here turned back into this process's addresses, which must be every page of what xnudrmd had
 * translated, in order. *id: the map's id. Returns the address, NULL when the map failed (the status is in R).
 */
static uint8_t *kmap(uint32_t c, uint32_t handle, uint64_t size, uint64_t *id)
{
	unsigned x0 = n_xlate;
	if (rpc(XNUDRM_MAP, c, handle, size, NULL, 0)) { EQ(R->payload_len, 0); return NULL; }
	struct xnudrm_range *rg = (void *)RP;
	uint32_t nr = R->payload_len / sizeof(*rg);
	uint64_t at = 0, va0 = 0, total = 0;
	MUST(R->payload_len && R->payload_len % sizeof(*rg) == 0);
	for (uint32_t i = 0; i < nr; i++) {
		MUST(rg[i].len && !(rg[i].len & 4095));
		if (i) CHECK(rg[i - 1].pa + rg[i - 1].len != rg[i].pa);	/* one range per contiguous piece */
		for (uint64_t o = 0; o < rg[i].len; o += 4096, at += 4096) {
			uint64_t va = fake_va(rg[i].pa + o);
			if (!at) va0 = va;
			if (!va || va != va0 + at) { fail(__LINE__, "the mapping's page %llu is not the page xnudrmd mapped after page %llu", (unsigned long long)(at >> 12), (unsigned long long)(at >> 12) - 1); return NULL; }
		}
		total += rg[i].len;
	}
	EQ(total, PG(size));
	EQ(n_xlate, x0 + 1);
	CHECK(last_x.va == va0 && last_x.len == PG(size));
	if (id) *id = R->arg0;
	return (uint8_t *)(uintptr_t)va0;
}
static int kunmap(uint32_t c, uint64_t id) { return rpc(XNUDRM_UNMAP, c, id, 0, NULL, 0); }

/* ---- Venus commands (the wire format of the venus-protocol headers: 32-bit words, pointers as presence) ---- */

static uint32_t cs[2048];
static unsigned ncs;
static void e32(uint32_t v) { MUST(ncs < sizeof(cs) / 4); cs[ncs++] = v; }
static void e64(uint64_t v) { e32((uint32_t)v); e32((uint32_t)(v >> 32)); }
static void ecmd(uint32_t type, int reply) { e32(type); e32(reply); }
enum {
	vkCreateInstance = 0, vkDestroyInstance = 1, vkEnumeratePhysicalDevices = 2, vkGetPhysicalDeviceProperties = 6,
	vkGetPhysicalDeviceMemoryProperties = 8, vkCreateDevice = 11, vkDestroyDevice = 12, vkAllocateMemory = 21, vkFreeMemory = 22,
	vkEnumerateInstanceVersion = 137, vkGetDeviceQueue2 = 155, vkSetReplyCommandStreamMESA = 178, vkSeekReplyCommandStreamMESA = 179,
	vkCreateRingMESA = 188, vkDestroyRingMESA = 189, vkNotifyRingMESA = 190, vkWriteRingExtraMESA = 191, vkWaitRingSeqnoMESA = 253,
};

/* A ring as vn_ring.c lays one out: head, tail and status a cache line apart, the buffer, 8 bytes of extra. */
#define RING_BUF 65536u
enum { R_HEAD = 0, R_TAIL = 64, R_STATUS = 128, R_BUF = 192, R_EXTRA = R_BUF + RING_BUF, RING_SIZE = R_EXTRA + 8 };
#define REPLY_SIZE (1u << 20)
struct ring { uint32_t c, res, reply_res, tail, extra; uint64_t id; uint8_t *p; uint32_t *reply; };
#define AT(rg, off) (*(volatile uint32_t *)((rg)->p + (off)))

static struct ring ring_create(uint32_t c, uint64_t id)
{
	struct ring rg = { .c = c, .id = id, .res = shm(c, RING_SIZE) };
	MUST((rg.p = kmap(c, rg.res, RING_SIZE, NULL)));
	ncs = 0; ecmd(vkCreateRingMESA, 0); e64(id); e64(1);
	e32(1000384000); e64(0); e32(0); e32(rg.res); e64(0); e64(RING_SIZE); e64(5000000 /* idle after 5 ms */);
	e64(R_HEAD); e64(R_TAIL); e64(R_STATUS); e64(R_BUF); e64(RING_BUF); e64(R_EXTRA); e64(8);
	EQ(execbuf(c, cs, ncs * 4, VIRTGPU_EXECBUF_RING_IDX, 0, NULL), 0);
	return rg;
}

/* cs goes into the ring's buffer and the tail past it; a ring gone idle is woken (vn_ring_submit). Then until the renderer has done all of it. */
static int ring_run(struct ring *rg)
{
	double end = now_ms() + 120000;	/* (a laptop's second GPU takes its time to wake up for vkCreateInstance) */
	for (unsigned i = 0; i < ncs * 4; i++) rg->p[R_BUF + ((rg->tail + i) & (RING_BUF - 1))] = ((uint8_t *)cs)[i];
	rg->tail += ncs * 4;
	__atomic_store_n((uint32_t *)(rg->p + R_TAIL), rg->tail, __ATOMIC_RELEASE);
	if (AT(rg, R_STATUS) & 1) {
		uint32_t n[6] = { vkNotifyRingMESA, 0, (uint32_t)rg->id, (uint32_t)(rg->id >> 32), rg->tail, 0 };
		EQ(execbuf(rg->c, n, sizeof(n), VIRTGPU_EXECBUF_RING_IDX, 0, NULL), 0);
	}
	while (AT(rg, R_HEAD) != rg->tail) {
		if ((AT(rg, R_STATUS) & 2) || now_ms() > end) { fail(__LINE__, "the ring %s: head %u, tail %u, status %#x", AT(rg, R_STATUS) & 2 ? "is dead" : "does not move", AT(rg, R_HEAD), rg->tail, AT(rg, R_STATUS)); return -1; }
		usleep(50);
	}
	return 0;
}
/* One command with a reply, which is then at the start of the ring's reply memory. */
static void rcmd(uint32_t type) { ncs = 0; ecmd(vkSeekReplyCommandStreamMESA, 0); e64(0); ecmd(type, 1); }
static void ring_reply_memory(struct ring *rg)
{
	rg->reply_res = shm(rg->c, REPLY_SIZE);
	MUST((rg->reply = (uint32_t *)kmap(rg->c, rg->reply_res, REPLY_SIZE, NULL)));
	ncs = 0; ecmd(vkSetReplyCommandStreamMESA, 0); e64(1); e32(rg->reply_res); e64(0); e64(REPLY_SIZE);
	MUST(ring_run(rg) == 0);
}
/* vn_instance_submit_roundtrip: a value written past the ring from a submit, to know that submit has been done. */
static int roundtrip(struct ring *rg)
{
	double end = now_ms() + 20000;
	ncs = 0; ecmd(vkWriteRingExtraMESA, 0); e64(rg->id); e64(0); e32(++rg->extra);
	EQ(execbuf(rg->c, cs, ncs * 4, VIRTGPU_EXECBUF_RING_IDX, 0, NULL), 0);
	while (AT(rg, R_EXTRA) != rg->extra) { if (now_ms() > end) return -1; usleep(50); }
	return 0;
}
static struct ring client_ring(uint32_t *c)
{
	*c = xopen();
	MUST(context_init(*c, 4) == 0);
	struct ring rg = ring_create(*c, 0x5000 + *c);
	ring_reply_memory(&rg);
	return rg;
}

/* ---- what is left behind: descriptors, mappings, the renderer's processes ---- */

static int count_dir(const char *path)
{
	DIR *d = opendir(path);
	int n = 0;
	if (!d) return -1;
	while (readdir(d)) n++;
	closedir(d);
	return n - 2 - !strcmp(path, "/proc/self/fd");	/* ".", "..", and the listing's own descriptor */
}
static int my_fds(void) { return count_dir("/proc/self/fd"); }
static long rss_kb(pid_t pid)
{
	char path[64], line[256];
	long kb = -1;
	snprintf(path, sizeof(path), "/proc/%d/status", pid);
	FILE *f = fopen(path, "r");
	while (f && fgets(line, sizeof(line), f)) if (sscanf(line, "VmRSS: %ld", &kb) == 1) break;
	if (f) fclose(f);
	return kb;
}
/* The processes below this one (the render server and a worker per context): how many, their descriptors, memory, ids. */
static int below(pid_t top, int *fds, long *kb, pid_t *pids, int max)
{
	DIR *d = opendir("/proc");
	struct dirent *e;
	int n = 0;
	while (d && (e = readdir(d))) {
		char path[300];
		int pid = atoi(e->d_name), ppid = 0;
		if (pid <= 0) continue;
		snprintf(path, sizeof(path), "/proc/%d/stat", pid);
		FILE *f = fopen(path, "r");
		if (!f) continue;
		if (fscanf(f, "%*d (%*[^)]) %*c %d", &ppid) == 1 && ppid == top) {
			snprintf(path, sizeof(path), "/proc/%d/fd", pid);
			if (fds) *fds += count_dir(path);
			if (kb) *kb += rss_kb(pid);
			if (pids && n < max) pids[n] = pid;
			n++;
			n += below(pid, fds, kb, pids ? pids + n : NULL, max - n);
		}
		fclose(f);
	}
	if (d) closedir(d);
	return n;
}
/* (a worker goes a moment after its context) */
static int renderers(int want)
{
	int n = 0;
	for (int i = 0; i < 500 && (n = below(getpid(), NULL, NULL, NULL, 0)) != want; i++) usleep(10000);
	return n;
}

/* ---- the cases ---- */

/* xnudrmd as main() sets it up; 0 when the backend started. */
static int started(void)
{
	hc = HC; virgl = 1;
	msg = malloc(8 + XNUDRM_MAX_MSG + sizeof(struct xnudrm_hdr));
	rep = malloc(8 + XNUDRM_MAX_MSG + sizeof(struct xnudrm_hdr));
	return msg && rep ? vg_init() : -1;
}
static void start(void) { MUST(started() == 0); }

static void t_open(void)
{
	uint64_t v;
	start();
	EQ(rpc(XNUDRM_OPEN, 0, XNUDRM_VERSION + 1, 0, NULL, 0), -EPROTO);
	EQ(rpc(XNUDRM_OPEN, 0, 0, 0, NULL, 0), -EPROTO);
	uint32_t a = xopen(), b = xopen();
	CHECK(a != b);
	EQ(getparam(a, VIRTGPU_PARAM_3D_FEATURES, &v), 0);
	EQ(xclose(a), 0);
	EQ(getparam(a, VIRTGPU_PARAM_3D_FEATURES, &v), -EBADF);	/* closed is closed */
	EQ(xclose(a), -EBADF);
	EQ(rpc(XNUDRM_MAP, a, 1, 4096, NULL, 0), -EBADF);
	EQ(getparam(b, VIRTGPU_PARAM_3D_FEATURES, &v), 0);
	EQ(xopen(), a);	/* the id is free again */
	EQ(rpc(99, b, 0, 0, NULL, 0), -ENOSYS);
	EQ(xclose(b), 0); EQ(xclose(a), 0);
}

/* virtgpu_init_params: four that must be 1, where blobs live, blob alignment (only a newer kernel knows it), cross-device. */
static void t_getparam(void)
{
	static const struct { int param, st; uint64_t val; const char *name; } p[] = {
		{ VIRTGPU_PARAM_3D_FEATURES, 0, 1, "3D_FEATURES" }, { VIRTGPU_PARAM_CAPSET_QUERY_FIX, 0, 1, "CAPSET_QUERY_FIX" },
		{ VIRTGPU_PARAM_RESOURCE_BLOB, 0, 1, "RESOURCE_BLOB" }, { VIRTGPU_PARAM_CONTEXT_INIT, 0, 1, "CONTEXT_INIT" },
		{ VIRTGPU_PARAM_HOST_VISIBLE, 0, 1, "HOST_VISIBLE" }, { 9, -EINVAL, 0, "BLOB_ALIGNMENT" },
		{ VIRTGPU_PARAM_CROSS_DEVICE, 0, 0, "CROSS_DEVICE" }, { VIRTGPU_PARAM_SUPPORTED_CAPSET_IDs, 0, 1 << 4, "SUPPORTED_CAPSET_IDs" },
		{ 0, -EINVAL, 0, "0" }, { 1000, -EINVAL, 0, "1000" },
	};
	start();
	uint32_t c = xopen();
	for (unsigned i = 0; i < sizeof(p) / sizeof(p[0]); i++) {
		uint64_t v = 0;
		int r = getparam(c, p[i].param, &v);
		if (r != p[i].st || v != p[i].val) fail(__LINE__, "GETPARAM %s: status %d value %llu, not %d and %llu", p[i].name, r, (unsigned long long)v, p[i].st, (unsigned long long)p[i].val);
	}
	EQ(xclose(c), 0);
}

/* virtgpu_init_capset: the Venus capset, version 0, into the driver's own struct, whatever its size. */
static void t_caps(void)
{
	uint32_t ver = 0, size = 0, c;
	start();
	virgl_renderer_get_cap_set(4, &ver, &size);
	uint8_t *want = calloc(1, size + 64), *got = malloc(size + 64);
	virgl_renderer_fill_caps(4, 0, want);
	c = xopen();
	EQ(get_caps(c, 4, 0, got, size), 0);
	CHECK(!memcmp(got, want, size));
	uint32_t *w = (uint32_t *)got;	/* struct virgl_renderer_capset_venus: what Mesa refuses to run without */
	say("Venus capset: %u bytes, wire format %u, vk.xml %u.%u.%u, protocol %u, blob id 0 %u, vkWaitForFences on syncs %u, timelines %u", size, w[0],
	    w[1] >> 22, (w[1] >> 12) & 1023, w[1] & 4095, w[3], w[4], w[5 + 32], w[6 + 32]);
	CHECK(size >= 4 * (7 + 32) && w[0] && w[4] && w[5 + 32] && w[6 + 32]);
	memset(got, 0x77, size + 64);
	EQ(get_caps(c, 4, 0, got, 160), 0);	/* a driver whose struct is shorter */
	CHECK(!memcmp(got, want, 160 < size ? 160 : size));
	EQ(get_caps(c, 4, 0, got, size + 64), 0);	/* and one whose struct is longer: the rest stays as the driver had it */
	CHECK(!memcmp(got, want, size + 64));
	EQ(get_caps(c, 4, 0, got, 0), 0);
	EQ(get_caps(c, 1, 0, got, size), -EINVAL);	/* virgl's: not offered (SUPPORTED_CAPSET_IDs) */
	EQ(get_caps(c, 2, 0, got, size), -EINVAL);
	EQ(xclose(c), 0);
}

static void t_context(void)
{
	uint32_t h, f;
	start();
	uint32_t c = xopen();
	/* nothing of a context before there is one */
	EQ(blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 4096, 0, NULL, 0, &h), -EINVAL);
	EQ(execbuf(c, NULL, 0, 0, 0, NULL), -EINVAL);
	EQ(context_init(c, -1), -EINVAL);	/* no capset: a virgl (OpenGL) context, which this renderer has none of */
	EQ(context_init(c, 1), -EINVAL);
	EQ(context_init(c, 4), 0);
	EQ(context_init(c, 4), -EEXIST);
	EQ(execbuf(c, NULL, 0, 0, 0, &f), 0);
	h = shm(c, 4096);
	uint32_t d = xopen();	/* a second client's context is its own */
	EQ(context_init(d, 4), 0);
	EQ(renderers(3), 3);	/* the render server and a worker per context */
	EQ(xclose(c), 0);
	EQ(execbuf(d, NULL, 0, 0, 0, &f), 0);
	c = xopen();
	EQ(context_init(c, 4), 0);	/* the id again, with a new context */
	EQ(execbuf(c, NULL, 0, 0, 0, &f), 0);
	EQ(xclose(c), 0); EQ(xclose(d), 0);
}

static void t_blob(void)
{
	static const uint64_t sizes[] = { 1, 4096, 5000, 65736, 1 << 20, (3 << 20) + 123, 64 << 20 };
	uint32_t h[8], x;
	struct drm_virtgpu_resource_info info;
	start();
	uint32_t c = xopen();
	EQ(context_init(c, 4), 0);
	int fds = my_fds();
	for (unsigned i = 0; i < 7; i++) {
		h[i] = shm(c, sizes[i]);
		for (unsigned j = 0; j < i; j++) CHECK(h[i] != h[j]);
		EQ(res_info(c, h[i], &info), 0);
		CHECK(info.res_handle && info.size == sizes[i] && info.blob_mem == VIRTGPU_BLOB_MEM_HOST3D);
	}
	say("7 blobs of shared memory hold %d descriptors in xnudrmd", my_fds() - fds);
	/* what Venus never asks for: guest memory, no size, blob id 0 that is not just mappable (no such VkDeviceMemory) */
	EQ(blob_cmd(c, VIRTGPU_BLOB_MEM_GUEST, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 4096, 0, NULL, 0, &x), -EINVAL);
	EQ(blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 0, 0, NULL, 0, &x), -EINVAL);
	CHECK(blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE | VIRTGPU_BLOB_FLAG_USE_SHAREABLE, 4096, 0, NULL, 0, &x) < 0);
	CHECK(blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 4096, 12345, NULL, 0, &x) < 0);
	x = shm(c, 4096);	/* and the context still works after those */
	EQ(gem_close(c, x), 0);
	EQ(gem_close(c, x), -EINVAL);
	EQ(res_info(c, x, &info), -ENOENT);
	EQ(shm(c, 8192), x);	/* a closed handle is free again */
	EQ(gem_close(c, x), 0);
	for (unsigned i = 0; i < 7; i++) EQ(gem_close(c, h[i]), 0);
	EQ(my_fds(), fds);
	EQ(xclose(c), 0);
}

static void t_map(void)
{
	static const uint64_t sizes[] = { 1, 4096, 5000, 65736, 1 << 20, (3 << 20) + 123 };
	char name[256];
	start();
	uint32_t c = xopen();
	EQ(context_init(c, 4), 0);
	for (unsigned i = 0; i < sizeof(sizes) / sizeof(sizes[0]); i++) {
		uint64_t size = sizes[i], id = 0, id2 = 0;
		uint32_t h = shm(c, size);
		unsigned p0 = n_pin, u0 = n_unpin;
		uint8_t *p = kmap(c, h, size, &id);
		if (!p) { fail(__LINE__, "MAP of %llu bytes: %d", (unsigned long long)size, R->status); continue; }
		/* shared memory: pinned whole, once, before its addresses are handed out */
		EQ(n_pin, p0 + 1);
		CHECK(last_pin.va == (uintptr_t)p && last_pin.len == PG(size) && pinned((uintptr_t)p, PG(size)));
		vma((uintptr_t)p, PG(size), name, sizeof(name));
		if (!i) say("a blob of shared memory is a mapping of %s", name);
		for (uint64_t o = 0; o < size; o += 4096) { CHECK(p[o] == 0); p[o] = (uint8_t)(o >> 12) + 1; }
		p[size - 1] = 0x5a;
		/* again (the same pages, no second pin), a part of it, and more than there is */
		uint8_t *q = kmap(c, h, size, &id2);
		CHECK(q == p && id2 == id);
		EQ(n_pin, p0 + 1);
		CHECK(kmap(c, h, 1, NULL) == p);
		CHECK(!kmap(c, h, PG(size) + 1, NULL) && R->status == -EINVAL);
		EQ(rpc(XNUDRM_MAP, c, h, 0, NULL, 0), -EINVAL);
		EQ(kunmap(c, id), 0);
		EQ(kunmap(c, id), 0);	/* (the second mapping's) */
		EQ(n_unpin, u0);	/* the resource is still the client's: so are its pages */
		uint8_t *r = kmap(c, h, size, NULL);	/* mapped again after an unmap: what was written is there */
		CHECK(r && r[size - 1] == 0x5a && (size == 1 || r[0] == 1));
		EQ(gem_close(c, h), 0);
		EQ(n_unpin, u0 + 1);
		CHECK(!pinned((uintptr_t)p, 4096));
		CHECK(msync(p, 4096, MS_ASYNC) && errno == ENOMEM);	/* unmapped in xnudrmd */
		EQ(kunmap(c, id), -EINVAL);
	}
	uint32_t h = shm(c, 4096);
	EQ(kunmap(c, h), -EINVAL);	/* never mapped */
	EQ(xclose(c), 0);
	EQ(npins, 0); EQ(n_refused, 0);
}

/* Commands through the ring's memory and answers from the reply memory: both are mappings made here, of the renderer's pages. */
static void t_ring(void)
{
	uint32_t c;
	start();
	struct ring rg = client_ring(&c);
	for (int i = 0; i < 300; i++) {	/* (past the buffer's end and round) */
		rg.reply[0] = rg.reply[1] = rg.reply[4] = 0xdeadbeef;
		rcmd(vkEnumerateInstanceVersion); e64(1);
		MUST(ring_run(&rg) == 0);
		if (rg.reply[0] != vkEnumerateInstanceVersion || rg.reply[1] != 0 || rg.reply[4] < (1u << 22 | 1u << 12)) {
			fail(__LINE__, "round %d: the reply is %#x %#x .. %#x", i, rg.reply[0], rg.reply[1], rg.reply[4]);
			break;
		}
		if (i == 150) usleep(30000);	/* the ring goes idle and has to be woken */
	}
	say("vkEnumerateInstanceVersion through the ring: Vulkan %u.%u.%u", rg.reply[4] >> 22, (rg.reply[4] >> 12) & 1023, rg.reply[4] & 4095);
	EQ(roundtrip(&rg), 0);
	EQ(roundtrip(&rg), 0);
	/* vn_ring_destroy: the ring, then its memory; the context goes on */
	ncs = 0; ecmd(vkDestroyRingMESA, 0); e64(rg.id);
	EQ(execbuf(c, cs, ncs * 4, VIRTGPU_EXECBUF_RING_IDX, 0, NULL), 0);
	EQ(gem_close(c, rg.res), 0);
	EQ(gem_close(c, rg.reply_res), 0);
	rg = ring_create(c, 77);
	ring_reply_memory(&rg);
	rg.reply[0] = 0;
	rcmd(vkEnumerateInstanceVersion); e64(1);
	EQ(ring_run(&rg), 0);
	EQ(rg.reply[0], vkEnumerateInstanceVersion);
	EQ(xclose(c), 0);
	EQ(npins, 0); EQ(n_refused, 0);
}

static void t_fence(void)
{
	uint32_t f[64], x = 0;
	start();
	uint32_t c = xopen();
	EQ(context_init(c, 4), 0);
	EQ(execbuf(c, NULL, 0, 0, 0, &x), 0);
	EQ(execbuf(c, NULL, 0, VIRTGPU_EXECBUF_RING_IDX, 0, &x), 0);
	/* not over the bridge, and libxnudrm does not send them: */
	EQ(execbuf(c, NULL, 0, VIRTGPU_EXECBUF_FENCE_FD_IN, 0, &x), -EOPNOTSUPP);
	f[0] = fence_on(c, 0);
	EQ(fence_poll(c, f[0], 2000), 0);
	EQ(fence_wait(c, f[0], 0), 0);	/* signalled stays signalled */
	EQ(fence_wait(c, f[0], 1000000), 0);
	EQ(fence_close(c, f[0]), 0);
	EQ(fence_wait(c, f[0], 0), -EINVAL);
	EQ(fence_close(c, f[0]), -EINVAL);
	/* many at once, waited for in another order than made; one closed unwaited in between */
	for (int i = 0; i < 64; i++) { f[i] = fence_on(c, 0); for (int j = 0; j < i; j++) CHECK(f[i] != f[j]); }
	EQ(fence_close(c, f[10]), 0);
	for (int i = 63; i >= 0; i--) if (i != 10) { EQ(fence_poll(c, f[i], 2000), 0); EQ(fence_close(c, f[i]), 0); }
	/* the slot of a fence closed before it signalled is the next fence's: that one signals on its own account */
	for (int i = 0; i < 200; i++) {
		uint32_t a = fence_on(c, 0);
		EQ(fence_close(c, a), 0);
		uint32_t b = fence_on(c, 0);
		if (fence_poll(c, b, 2000)) { fail(__LINE__, "round %d: fence %u does not signal", i, b); break; }
		EQ(fence_close(c, b), 0);
	}
	EQ(xclose(c), 0);
}

/* A fence that stays pending (the renderer's processes are stopped): how long each wait holds the caller, a hosted CPU. */
static void t_fence_wait(void)
{
	static const uint64_t ns[] = { 0, 1000, 1000000, 10000000, 49000000, 50000000, 1000000000, 1ull << 40, 1ull << 63, ~0ull };
	pid_t pids[8];
	start();
	uint32_t c = xopen();
	EQ(context_init(c, 4), 0);
	uint32_t f = fence_on(c, 0);
	EQ(fence_poll(c, f, 2000), 0);
	EQ(fence_close(c, f), 0);
	int n = below(getpid(), NULL, NULL, pids, 8);
	MUST(n == 2);
	for (int i = 0; i < n; i++) kill(pids[i], SIGSTOP);
	f = fence_on(c, 0);
	double over = 1e9;
	for (unsigned i = 0; i < sizeof(ns) / sizeof(ns[0]); i++) {
		double t0 = now_ms(), want = ns[i] / 1e6 > 50 ? 50 : ns[i] / 1e6;	/* no host call is to take more than 50 ms */
		int r = fence_wait(c, f, ns[i]);
		double took = now_ms() - t0;
		EQ(r, -XNUDRM_ETIME);
		if (took < want || took > want + 2000) fail(__LINE__, "a wait of %llu ns for a pending fence took %.1f ms, not %.0f", (unsigned long long)ns[i], took, want);
		if (want >= 49 && took - want < over) over = took - want;
	}
	/* (the best of them: a busy machine may hold any one up) */
	if (over > 12) fail(__LINE__, "the waits of 49 ms and more for a pending fence each took at least %.1f ms longer than that", over);
	for (int i = 0; i < n; i++) kill(pids[i], SIGCONT);
	EQ(fence_poll(c, f, 2000), 0);
	EQ(fence_close(c, f), 0);
	EQ(xclose(c), 0);
}

/* ---- a Vulkan device behind the context ---- */

struct dev { uint64_t pd, id; char name[256]; uint32_t ring_idx, ntypes; struct { uint32_t flags, heap; } type[32]; uint64_t heap[16]; };
#define INSTANCE 1ull

static int devices(struct ring *rg, struct dev *dv, int max)
{
	uint32_t n;
	rcmd(vkCreateInstance); e64(1); e32(1); e64(0); e32(0);
	e64(1); e32(0); e64(0); e64(0); e32(0); e64(0); e32(0); e32(1u << 22 | 1u << 12);	/* application info: Vulkan 1.1 */
	e32(0); e64(0); e32(0); e64(0); e64(0); e64(1); e64(INSTANCE);
	MUST(ring_run(rg) == 0);
	if (rg->reply[0] != vkCreateInstance || rg->reply[1]) { say("vkCreateInstance: %d", (int)rg->reply[1]); return 0; }
	rcmd(vkEnumeratePhysicalDevices); e64(INSTANCE); e64(1); e32(0); e64(0);
	MUST(ring_run(rg) == 0);
	MUST(rg->reply[0] == vkEnumeratePhysicalDevices && rg->reply[1] == 0);
	n = rg->reply[4];
	if ((int)n > max) n = max;
	if (!n) return 0;
	rcmd(vkEnumeratePhysicalDevices); e64(INSTANCE); e64(1); e32(n); e64(n);
	for (uint32_t i = 0; i < n; i++) e64(dv[i].pd = 0x100 + i);
	MUST(ring_run(rg) == 0);
	MUST(rg->reply[0] == vkEnumeratePhysicalDevices && (int)rg->reply[1] >= 0 && rg->reply[4] == n);
	for (uint32_t i = 0; i < n; i++) {
		struct dev *d = &dv[i];
		rcmd(vkGetPhysicalDeviceProperties); e64(d->pd); e64(1);
		MUST(ring_run(rg) == 0 && rg->reply[0] == vkGetPhysicalDeviceProperties);
		snprintf(d->name, sizeof(d->name), "%s", (char *)&rg->reply[10]);
		if (rg->reply[7] != 4 /* VK_PHYSICAL_DEVICE_TYPE_CPU */) {
			static int told;
			if (!told++) say("%s: left alone (these cases use software devices only)", d->name);
			d->id = 0;
			continue;
		}
		rcmd(vkGetPhysicalDeviceMemoryProperties); e64(d->pd); e64(1); e64(32); e64(16);
		MUST(ring_run(rg) == 0 && rg->reply[0] == vkGetPhysicalDeviceMemoryProperties);
		d->ntypes = rg->reply[3];
		memcpy(d->type, &rg->reply[6], sizeof(d->type));
		for (int j = 0; j < 16; j++) memcpy(&d->heap[j], &rg->reply[73 + 3 * j], 8);
		/* a device with one queue, on a timeline of the context (ring index i + 1), as vn_device_init and vn_queue_init do */
		d->id = 0x200 + i; d->ring_idx = i + 1;
		rcmd(vkCreateDevice); e64(d->pd); e64(1); e32(3); e64(0); e32(0); e32(1); e64(1);
		e32(2); e64(0); e32(0); e32(0); e32(1); e64(1); e32(0x3f800000);
		e32(0); e64(0); e32(0); e64(0); e64(0); e64(0); e64(1); e64(d->id);
		MUST(ring_run(rg) == 0 && rg->reply[0] == vkCreateDevice);
		if (rg->reply[1]) { say("%s: vkCreateDevice: %d", d->name, (int)rg->reply[1]); d->id = 0; continue; }
		ncs = 0; ecmd(vkGetDeviceQueue2, 0); e64(d->id); e64(1); e32(1000145003); e64(1); e32(1000384005); e64(0); e32(d->ring_idx);
		e32(0); e32(0); e32(0); e64(1); e64(0x300 + i);
		MUST(ring_run(rg) == 0);
	}
	return n;
}

static uint64_t next_mem = 0x1000;
/* vkAllocateMemory on the ring; 0 when the device refused. */
static uint64_t alloc(struct ring *rg, struct dev *d, uint64_t size, uint32_t type, int wait)
{
	uint64_t id = next_mem++;
	if (wait) rcmd(vkAllocateMemory); else { ncs = 0; ecmd(vkAllocateMemory, 0); }
	e64(d->id); e64(1); e32(5); e64(0); e64(size); e32(type); e64(0); e64(1); e64(id);
	if (!wait) {	/* vn_device_memory_alloc_simple: not waited for; the blob's command waits (below) */
		for (unsigned i = 0; i < ncs * 4; i++) rg->p[R_BUF + ((rg->tail + i) & (RING_BUF - 1))] = ((uint8_t *)cs)[i];
		rg->tail += ncs * 4;
		__atomic_store_n((uint32_t *)(rg->p + R_TAIL), rg->tail, __ATOMIC_RELEASE);
		return id;
	}
	MUST(ring_run(rg) == 0 && rg->reply[0] == vkAllocateMemory);
	return rg->reply[1] ? 0 : id;
}
static void mfree(struct ring *rg, struct dev *d, uint64_t mem)
{
	ncs = 0; ecmd(vkFreeMemory, 0); e64(d->id); e64(mem); e64(0);
	EQ(ring_run(rg), 0);
}

/* One memory type of a device: memory allocated, made a blob, mapped, written, fenced on the device's queue, given back. */
static void device_memory(struct ring *rg, struct dev *d, uint32_t t, int quiet)
{
	static const uint64_t sizes[] = { 4096, 65536, 100000, (1 << 20) + 4096, 64 << 20 };
	uint32_t c = rg->c;
	char name[256];
	for (unsigned s = 0; s < sizeof(sizes) / sizeof(sizes[0]); s++) {
		uint64_t size = sizes[s], id = 0;
		uint32_t h = 0, f[4], wait[6] = { vkWaitRingSeqnoMESA, 0, (uint32_t)rg->id, (uint32_t)(rg->id >> 32), 0, 0 };
		unsigned p0 = n_pin, u0 = n_unpin;
		struct drm_virtgpu_resource_info info;
		uint64_t mem = alloc(rg, d, size, t, s & 1);
		if (!mem) { say("%s: memory type %u: no %llu bytes", d->name, t, (unsigned long long)size); continue; }
		wait[4] = rg->tail;
		/* vn_device_memory_bo_init: the blob is the memory's id; with the allocation not yet waited for, a command that waits for it */
		int r = blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, size, mem, s & 1 ? NULL : wait, s & 1 ? 0 : sizeof(wait), &h);
		if (r) { fail(__LINE__, "%s: memory type %u (flags %#x), %llu bytes: RESOURCE_CREATE_BLOB %d", d->name, t, d->type[t].flags, (unsigned long long)size, r); mfree(rg, d, mem); continue; }
		EQ(res_info(c, h, &info), 0);
		CHECK(info.size == size && info.blob_mem == VIRTGPU_BLOB_MEM_HOST3D);
		uint8_t *p = kmap(c, h, size, &id);
		if (!p) { fail(__LINE__, "%s: memory type %u (flags %#x), %llu bytes: MAP %d", d->name, t, d->type[t].flags, (unsigned long long)size, R->status); gem_close(c, h); mfree(rg, d, mem); continue; }
		int dev = vma((uintptr_t)p, PG(size), name, sizeof(name));
		/* asked to be pinned once, whole; pinned unless it is a device's own mapping */
		EQ(n_pin, p0 + 1);
		CHECK(last_pin.va == (uintptr_t)p && last_pin.len >= PG(size) && pinned((uintptr_t)p, PG(size)) == !dev);
		if (!s && !quiet) say("%s: memory type %u (flags %#x) is a mapping of %s, %s", d->name, t, d->type[t].flags, name, dev ? "a device's: not pinned" : "pinned");
		for (uint64_t o = 0; o < size; o += 4096) p[o] = (uint8_t)(o >> 12) + 1;
		p[size - 1] = 0x5a;
		for (uint64_t o = 4096; o < size; o += 4096) if (p[o] != (uint8_t)((o >> 12) + 1)) { fail(__LINE__, "%s: the mapping does not keep what is written", d->name); break; }
		/* fences on the device's queue, the last made asked about first */
		for (int k = 0; k < 4; k++) f[k] = fence_on(c, d->ring_idx);
		for (int k = 3; k >= 0; k--) { EQ(fence_poll(c, f[k], 5000), 0); EQ(fence_close(c, f[k]), 0); }
		/* virtgpu_bo_destroy, then vkFreeMemory */
		EQ(kunmap(c, id), 0);
		EQ(gem_close(c, h), 0);
		EQ(n_unpin, u0 + !dev);
		mfree(rg, d, mem);
	}
}

/* Every host-visible memory type of every device the renderer has. */
static void t_device_memory(void)
{
	struct dev dv[8];
	uint32_t c;
	start();
	struct ring rg = client_ring(&c);
	int n = devices(&rg, dv, 8), fds = 0;
	if (!n) { say("the renderer has no Vulkan device here: not tested"); return; }
	/* twice: what the first time leaves open (the renderer's own device, to map memory that is not a dma-buf) is all the second leaves */
	for (int pass = 0; pass < 2; pass++) {
		for (int i = 0; i < n; i++)
			for (uint32_t t = 0; dv[i].id && t < dv[i].ntypes; t++)
				if (dv[i].type[t].flags & 2 /* host visible */) device_memory(&rg, &dv[i], t, pass);
		EQ(roundtrip(&rg), 0);
		if (pass) EQ(my_fds(), fds); else fds = my_fds();
	}
	EQ(npins, 2);	/* the ring and its reply memory */
	for (int i = 0; i < n; i++) if (dv[i].id) { ncs = 0; ecmd(vkDestroyDevice, 0); e64(dv[i].id); e64(0); EQ(ring_run(&rg), 0); }
	ncs = 0; ecmd(vkDestroyInstance, 0); e64(INSTANCE); e64(0); EQ(ring_run(&rg), 0);
	EQ(xclose(c), 0);
	EQ(npins, 0); EQ(n_refused, 0);
}

/* Over and over what a client does all day: nothing may be left of a round, here or in the renderer's processes. */
static void t_rounds(void)
{
	uint32_t c;
	int fds[3], rfds[3] = { 0 }, n;
	long kb[3], rkb[3] = { 0 };
	start();
	struct ring rg = client_ring(&c);
	for (int pass = 0; pass < 3; pass++) {
		for (int i = 0; i < 2000; i++) {
			uint64_t id;
			uint32_t h = shm(c, 4096 + 4096 * (i % 16)), f = fence_on(c, 0);
			uint8_t *p = kmap(c, h, 4096, &id);
			if (!p || fence_poll(c, f, 2000)) { fail(__LINE__, "round %d: map %p", i, p); break; }
			p[0] = 1;
			EQ(fence_close(c, f), 0);
			f = fence_on(c, 0);	/* and one that is never waited for */
			EQ(fence_close(c, f), 0);
			EQ(kunmap(c, id), 0);
			EQ(gem_close(c, h), 0);
			if (bad) break;
		}
		EQ(roundtrip(&rg), 0);
		fds[pass] = my_fds(); kb[pass] = rss_kb(getpid());
		n = below(getpid(), &rfds[pass], &rkb[pass], NULL, 0);
		say("after %d rounds: xnudrmd %d descriptors, %ld kB; the renderer's %d processes %d descriptors, %ld kB", 2000 * (pass + 1), fds[pass], kb[pass], n, rfds[pass], rkb[pass]);
	}
	EQ(fds[2], fds[0]); EQ(rfds[2], rfds[0]);
	CHECK(kb[2] - kb[1] < 1024 && rkb[2] - rkb[1] < 1024);
	EQ(xclose(c), 0);
	EQ(npins, 0); EQ(n_refused, 0);
}

/* A process that died: the kext closes its client with everything still there (clientClose). Nothing may be left, and the next client is not the worse for it. */
static void t_crashed(void)
{
	struct dev dv[8];
	uint32_t c, c2;
	int fds0 = 0;
	start();
	for (int round = 0; round < 4; round++) {
		struct ring rg = client_ring(&c), rg2 = client_ring(&c2);
		int n = devices(&rg, dv, 8);
		uint8_t *p[4];
		for (int i = 0; i < 4; i++) { uint32_t h = shm(c, 1 << 20); MUST((p[i] = kmap(c, h, 1 << 20, NULL))); p[i][0] = 1; }
		for (int i = 0; i < n; i++) {
			if (!dv[i].id) continue;
			for (uint32_t t = 0; t < dv[i].ntypes; t++) {
				uint32_t h;
				uint64_t mem;
				if (!(dv[i].type[t].flags & 2) || !(mem = alloc(&rg, &dv[i], 1 << 20, t, 1))) continue;
				if (blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 1 << 20, mem, NULL, 0, &h) == 0) kmap(c, h, 1 << 20, NULL);
			}
			fence_on(c, dv[i].ring_idx);
		}
		for (int i = 0; i < 10; i++) fence_on(c, 0);
		uint32_t f2 = fence_on(c2, 0);
		rcmd(vkEnumerateInstanceVersion); e64(1);	/* the ring busy while its client goes */
		for (unsigned i = 0; i < ncs * 4; i++) rg.p[R_BUF + ((rg.tail + i) & (RING_BUF - 1))] = ((uint8_t *)cs)[i];
		__atomic_store_n((uint32_t *)(rg.p + R_TAIL), rg.tail + ncs * 4, __ATOMIC_RELEASE);
		EQ(xclose(c), 0);
		CHECK(msync(p[0], 4096, MS_ASYNC) && errno == ENOMEM);
		EQ(renderers(2), 2);	/* the dead client's worker is gone */
		/* the other client never noticed */
		EQ(fence_poll(c2, f2, 2000), 0);
		rcmd(vkEnumerateInstanceVersion); e64(1);
		EQ(ring_run(&rg2), 0);
		EQ(roundtrip(&rg2), 0);
		EQ(xclose(c2), 0);
		EQ(npins, 0);
		if (!round) fds0 = my_fds(); else EQ(my_fds(), fds0);	/* (the first round leaves the renderer's own, as above) */
	}
	EQ(n_refused, 0);
}

/* As many clients as there are ids, each with a context, a ring and a fence; then one more. */
static void t_clients(void)
{
	static struct ring rg[256];
	static uint32_t c[256], f[256];
	int n;
	start();
	int fds0 = my_fds();
	for (n = 0; n < 255; n++) {
		rg[n] = client_ring(&c[n]);
		f[n] = fence_on(c[n], 0);
		if (bad) return;
	}
	say("255 clients with a context and a ring each: %d descriptors in xnudrmd, %d renderer processes", my_fds() - fds0, renderers(256));
	EQ(rpc(XNUDRM_OPEN, 0, XNUDRM_VERSION, 0, NULL, 0), -EMFILE);
	for (n = 0; n < 255; n += 7) {	/* each still its own */
		rg[n].reply[0] = 0;
		rcmd(vkEnumerateInstanceVersion); e64(1);
		EQ(ring_run(&rg[n]), 0);
		EQ(rg[n].reply[0], vkEnumerateInstanceVersion);
		EQ(fence_poll(c[n], f[n], 2000), 0);
		EQ(fence_wait(c[n], f[(n + 1) % 255], 0), -EINVAL);
	}
	EQ(xclose(c[100]), 0);
	EQ(xopen(), c[100]);
	for (n = 0; n < 255; n++) EQ(xclose(c[n]), 0);
	EQ(my_fds(), fds0);
	EQ(renderers(1), 1);
	EQ(npins, 0); EQ(n_refused, 0);
}

/* The tables: 65535 fences and 65535 resources over all clients; the next is refused, and one given back is enough. */
static void t_tables(void)
{
	uint32_t f = 0, h = 0, first = 0;
	struct rlimit rl;
	start();
	uint32_t c = xopen();
	EQ(context_init(c, 4), 0);
	for (int i = 1; i < 65536; i++) { f = fence_on(c, 0); if (!first) first = f; if (bad) return; }
	EQ(execbuf(c, NULL, 0, VIRTGPU_EXECBUF_RING_IDX | VIRTGPU_EXECBUF_FENCE_FD_OUT, 0, &f), -EMFILE);
	EQ(fence_poll(c, first, 5000), 0);
	EQ(fence_close(c, first), 0);
	EQ(fence_on(c, 0), first);
	EQ(xclose(c), 0);
	c = xopen();
	EQ(context_init(c, 4), 0);
	EQ(fence_poll(c, fence_on(c, 0), 2000), 0);	/* all free again */
	/* each resource is a descriptor here and, being shared memory, a mapping in the renderer's process */
	FILE *mc = fopen("/proc/sys/vm/max_map_count", "r");
	long maps = 0;
	if (mc) { if (fscanf(mc, "%ld", &maps) != 1) maps = 0; fclose(mc); }
	getrlimit(RLIMIT_NOFILE, &rl);
	if (rl.rlim_cur < 65536 + 64 || maps < 70000) {
		say("resources: not tested (this process may have %llu descriptors, one of the renderer's %ld mappings)", (unsigned long long)rl.rlim_cur, maps);
		EQ(xclose(c), 0);
		return;
	}
	for (int i = 1; i < 65536; i++) { EQ(blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 4096, 0, NULL, 0, &h), 0); if (bad) return; }
	EQ(blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 4096, 0, NULL, 0, &f), -ENOMEM);
	EQ(gem_close(c, h), 0);
	EQ(shm(c, 4096), h);
	EQ(xclose(c), 0);
}

/*
 * xnudrmd holds a descriptor for every blob (its memory) and two for every client, and scripts/xnulinux-boot.sh starts
 * it with systemd-run: with 1024 descriptors, unless it takes more.
 */
static void t_descriptors(void)
{
	static uint32_t h[3000];
	struct rlimit rl;
	int n;
	getrlimit(RLIMIT_NOFILE, &rl);
	if (rl.rlim_max < 4096) { say("not tested: no more than %llu descriptors to be had here", (unsigned long long)rl.rlim_max); return; }
	rl.rlim_cur = 1024;
	setrlimit(RLIMIT_NOFILE, &rl);
	start();
	uint32_t c = xopen();
	EQ(context_init(c, 4), 0);
	for (n = 0; n < 3000; n++) {
		int r = blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 4096, 0, NULL, 0, &h[n]);
		if (r) { fail(__LINE__, "blob %d: RESOURCE_CREATE_BLOB %d", n + 1, r); break; }
	}
	if (rpc(XNUDRM_OPEN, 0, XNUDRM_VERSION, 0, NULL, 0) == 0) EQ(context_init(R->client, 4), 0);	/* and a client after them */
	else fail(__LINE__, "OPEN %d", R->status);
	for (int i = 0; i < n; i++) EQ(gem_close(c, h[i]), 0);
	EQ(xclose(c), 0);
	/* a device's memory: the descriptors its driver takes for an allocation are the render server's (lavapipe: three each) */
	struct dev dv[8];
	struct ring rg = client_ring(&c);
	int nd = devices(&rg, dv, 8), di = 0;
	while (di < nd && !dv[di].id) di++;
	if (di < nd) {
		struct dev *d = &dv[di];
		uint32_t t = 0, x;
		while (t < d->ntypes && !(d->type[t].flags & 2)) t++;
		for (n = 0; n < 1500 && t < d->ntypes; n++) {
			uint64_t mem = alloc(&rg, d, 4096, t, 1);
			int r = mem ? blob_cmd(c, VIRTGPU_BLOB_MEM_HOST3D, VIRTGPU_BLOB_FLAG_USE_MAPPABLE, 4096, mem, NULL, 0, &x) : -ENOMEM;
			if (r) { fail(__LINE__, "%s: allocation %d: %s %d", d->name, n + 1, mem ? "RESOURCE_CREATE_BLOB" : "vkAllocateMemory", mem ? r : (int)rg.reply[1]); break; }
		}
	}
	EQ(xclose(c), 0);
}

int main(int argc, char **argv)
{
	static const struct { const char *name; void (*f)(void); int secs; } tests[] = {
		{ "open and close", t_open, 60 }, { "GETPARAM", t_getparam, 60 }, { "GET_CAPS", t_caps, 60 }, { "CONTEXT_INIT", t_context, 60 },
		{ "blobs of shared memory", t_blob, 60 }, { "MAP", t_map, 60 }, { "a ring", t_ring, 120 }, { "fences", t_fence, 120 },
		{ "FENCE_WAIT's timeout", t_fence_wait, 120 }, { "device memory", t_device_memory, 900 },
		{ "rounds", t_rounds, 600 }, { "a client that died", t_crashed, 900 }, { "255 clients", t_clients, 600 },
		{ "the tables", t_tables, 600 }, { "more blobs than 1024 descriptors", t_descriptors, 300 },
	};
	int failed = 0, st, r;
	pid_t pid;

	setvbuf(stdout, NULL, _IOLBF, 0);
	/*
	 * Is there a renderer to test with? One without Venus does not start (xnudrmd says so); one without its render
	 * server (Ubuntu 24.04's 1.0.0) starts and then has no context to give.
	 */
	if ((pid = fork()) < 0) { perror("fork"); return 1; }
	if (!pid) _exit(started() || rpc(XNUDRM_OPEN, 0, XNUDRM_VERSION, 0, NULL, 0) || context_init(R->client, 4) ? 77 : 0);
	do r = waitpid(pid, &st, 0); while (r < 0 && errno == EINTR);
	if (r == pid && WIFEXITED(st) && WEXITSTATUS(st) == 77) {
		printf("xnudrmd's virglrenderer backend: NOT TESTED: the virglrenderer here gives no Venus context (it takes 1.2 or newer, built with Venus, and its render server)\n");
		return 0;
	}
	for (unsigned i = 0; i < sizeof(tests) / sizeof(tests[0]); i++) {
		if (argc > 1 && !strstr(tests[i].name, argv[1])) continue;
		printf("%s\n", tests[i].name);
		fflush(stdout);
		if ((pid = fork()) < 0) { perror("fork"); return 1; }
		if (!pid) { alarm(tests[i].secs); tests[i].f(); fflush(stdout); _exit(!!bad); }
		do r = waitpid(pid, &st, 0); while (r < 0 && errno == EINTR);
		if (r != pid) { perror("waitpid"); return 1; }
		printf("%s: %s\n", tests[i].name, !st ? "ok" : WIFSIGNALED(st) ? strsignal(WTERMSIG(st)) : "FAIL");
		failed += !!st;
	}
	return !!failed;
}

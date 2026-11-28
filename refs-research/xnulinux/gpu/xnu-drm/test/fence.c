// SPDX-License-Identifier: MIT
/*
 * libxnudrm's fences against a stub of xnudrmd, with and without a strict sandbox (its denials are simulated). Runs on
 * Linux and macOS: see run.sh.
 */
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

static int sandbox;	/* deny what com.apple.WebKit.GPUProcess.sb denies */
static int denials, full;	/* full: no descriptor to be had */
static int deny(void)
{
	if (sandbox) { denials++; errno = EPERM; }
	return sandbox;
}
static int no_fd(void) { if (full) errno = EMFILE; return full; }
static int t_pipe(int *p) { return deny() || no_fd() ? -1 : pipe(p); }
static int t_poll(struct pollfd *f, nfds_t n, int ms) { return deny() ? -1 : poll(f, n, ms); }
static int t_open(const char *path, int flags, ...)
{
	return !strcmp(path, "/dev/null") && deny() ? -1 : no_fd() ? -1 : open(path, flags, 0600);
}
static int t_fcntl(int fd, int cmd, int arg) { return cmd == F_SETFD && no_fd() ? -1 : fcntl(fd, cmd, arg); }
int os_dupfd_cloexec(int fd) { return deny() ? -1 : fcntl(fd, F_DUPFD_CLOEXEC, 3); }

int xnudrm_munmap(void *addr, size_t len);	/* in the driver, the compat header declares it */
#define pipe t_pipe
#define poll t_poll
#define open t_open
#define fcntl t_fcntl
#include "../darwin/xnudrm.c"
#undef pipe
#undef poll
#undef open
#undef fcntl

/* ---- the stub: fence ids count up, signalled[] says which are */
static int next_id = 1, signalled[4096], closed[4096], broken;
int IOServiceOpen(io_service_t sv, int task, uint32_t type, io_connect_t *c)
{
	static io_connect_t next = 1;
	(void)sv; (void)task; (void)type;
	*c = next++;
	return 0;
}
int IOServiceClose(io_connect_t c) { (void)c; return 0; }
int IOConnectCallScalarMethod(io_connect_t c, uint32_t sel, const uint64_t *in, uint32_t nin, uint64_t *out, uint32_t *nout)
{
	struct xnudrm_hdr *h = (void *)buf;
	(void)c; (void)sel; (void)in; (void)nin; (void)out; (void)nout;
	h->status = 0;
	if (h->op == XNUDRM_IOCTL) ((struct drm_virtgpu_execbuffer *)(h + 1))->fence_fd = next_id++;
	if (h->op == XNUDRM_FENCE_WAIT && !signalled[h->arg0]) h->status = -XNUDRM_ETIME;
	if (h->op == XNUDRM_FENCE_WAIT && broken) return 1;
	if (h->op == XNUDRM_FENCE_CLOSE) closed[h->arg0]++;
	return 0;
}

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "fence.c:%d: %s\n", __LINE__, #x); _exit(1); } } while (0)
static int node(void)
{
	int n = xnudrm_open("/dev/dri/renderD128", O_RDWR);
	CHECK(n >= 0);
	return n;
}
static int raw_submit(int n)
{
	struct drm_virtgpu_execbuffer a = { .flags = VIRTGPU_EXECBUF_FENCE_FD_OUT };
	CHECK(drmIoctl(n, DRM_IOCTL_VIRTGPU_EXECBUFFER, &a) == 0 && a.fence_fd >= 0);
	return a.fence_fd;
}
static int submit(int n)	/* a fence the way Venus keeps one: imported (dup'ed), the original closed */
{
	int f = raw_submit(n), fd = xnudrm_dupfd_cloexec(f);
	CHECK(fd >= 0 && xnudrm_close(f) == 0);
	return fd;
}
#ifdef __APPLE__
#define READY POLLIN
#else
#define READY (POLLIN | POLLHUP)	/* a pipe at end of file: POLLIN as well on macOS, POLLHUP alone on Linux */
#endif
static int wait_fd(int fd, int ms)
{
	struct pollfd p = { .fd = fd, .events = POLLIN };
	int r = xnudrm_poll(&p, 1, ms);
	CHECK(r >= 0 && (r == 1) == !!(p.revents & READY));
	return r;
}

static void t_signal(void)
{
	int n = node(), f = submit(n), g = xnudrm_dupfd_cloexec(f);
	CHECK(wait_fd(f, 0) == 0 && wait_fd(g, 10) == 0);
	signalled[1] = 1;
	CHECK(wait_fd(f, 0) == 1 && wait_fd(g, -1) == 1 && wait_fd(f, 0) == 1);
	CHECK(closed[1] == 1);
	CHECK(xnudrm_close(f) == 0 && xnudrm_close(g) == 0 && closed[1] == 1);
}
static void t_close(void)	/* a fence's last descriptor closes it in xnudrmd */
{
	int n = node(), f = submit(n), g = xnudrm_dupfd_cloexec(f);
	xnudrm_close(f);
	CHECK(closed[1] == 0 && wait_fd(g, 0) == 0);
	xnudrm_close(g);
	CHECK(closed[1] == 1 && nsfds == 0 && nfences == 0);
}
static void t_unwaited(void)	/* fences dropped unwaited (vkResetFences, vkDestroyFence): nothing of one stays, however many */
{
	int n = node();
	struct rlimit few = { 64, 64 };
	CHECK(setrlimit(RLIMIT_NOFILE, &few) == 0);
	for (int i = 1; i <= 300; i++) CHECK(xnudrm_close(submit(n)) == 0 && closed[i] == 1);
	CHECK(nfences == 0 && nsfds == 0);
}
static void t_node_close(void)	/* the order of vkDestroyInstance: fences nobody waited for, then the node */
{
	int n = node(), f = submit(n), kept = submit(n);
	xnudrm_close(f);
	xnudrm_close(n);
	CHECK(wait_fd(kept, 1000) == 1);	/* a kept one is ready, not stuck */
	xnudrm_close(kept);
}
static void t_stale_new(void)	/* an exported fence fd closed by the application itself, its number used again */
{
	int n = node(), f = raw_submit(n), g;
	signalled[1] = 1;
	close(f);
	g = raw_submit(n);
	CHECK(wait_fd(g, 0) == 0);
	CHECK(xnudrm_close(g) == 0);
	CHECK(nsfds == 0);
}
static void t_stale_dup(void)	/* the same, the number used again by a dup */
{
	int n = node(), f = raw_submit(n), g = raw_submit(n), h;
	signalled[1] = 1;
	close(f);
	h = xnudrm_dupfd_cloexec(g);
	CHECK(h == f || !sandbox);
	CHECK(wait_fd(h, 0) == 0);
}
static void t_multi(void)
{
	int n = node();
	struct pollfd p[3] = { { .events = POLLIN }, { .fd = -1, .events = POLLIN, .revents = POLLIN }, { .events = POLLIN } };
	p[0].fd = submit(n);
	p[2].fd = submit(n);
	CHECK(xnudrm_poll(p, 3, 5) == 0 && !p[0].revents && !p[1].revents && !p[2].revents);
	signalled[2] = 1;
	CHECK(xnudrm_poll(p, 3, -1) == 1 && !p[0].revents && !p[1].revents && (p[2].revents & POLLIN));
}
static void t_mixed(void)	/* a sandbox fence beside another descriptor: native poll() says nothing of the fence */
{
	if (!sandbox) return;
	int n = node(), other[2];
	struct pollfd p[2] = { { .fd = submit(n), .events = POLLIN }, { .events = POLLIN } };
	CHECK(pipe(other) == 0);
	p[1].fd = other[0];
	sandbox = 0;
	CHECK(xnudrm_poll(p, 2, 5) == 0);
	CHECK(write(other[1], "x", 1) == 1);
	CHECK(xnudrm_poll(p, 2, -1) == 1 && !p[0].revents && (p[1].revents & POLLIN));
	signalled[1] = 1;
	CHECK(xnudrm_poll(p, 2, -1) == 2 && (p[0].revents & POLLIN) && (p[1].revents & POLLIN));
	p[0].events = 0;
	CHECK(xnudrm_poll(p, 1, 5) == 0);
}
static void t_error(void)	/* a failed wait is neither completion nor the end of the fence */
{
	int n = node(), f = submit(n);
	struct pollfd p = { .fd = f, .events = POLLIN };
	broken = 1;
	for (int i = 0; i < 2; i++) CHECK(xnudrm_poll(&p, 1, -1) == -1 && errno == EIO && !p.revents);
	broken = 0;
	CHECK(closed[1] == 0 && wait_fd(f, 0) == 0);
	signalled[1] = 1;
	CHECK(wait_fd(f, 0) == 1);
}
static void t_full(void)	/* no descriptor for the fence: the ioctl fails, and xnudrmd's fence is closed */
{
	int n = node();
	struct drm_virtgpu_execbuffer a = { .flags = VIRTGPU_EXECBUF_FENCE_FD_OUT };
	full = 1;
	errno = 0;
	CHECK(drmIoctl(n, DRM_IOCTL_VIRTGPU_EXECBUFFER, &a) == -1 && errno == EMFILE && closed[1] == 1);
	full = 0;
	CHECK(wait_fd(submit(n), 0) == 0);
}
static void t_dup_flags(void)
{
	int n = node(), f = raw_submit(n), err = fcntl(2, F_DUPFD_CLOEXEC, 100);
	full = sandbox;		/* F_SETFD fails: no descriptor rather than one without the flag */
	if (full) CHECK(xnudrm_dupfd_cloexec(f) == -1 && errno == EMFILE && nsfds == 1 && sfds[0].f->refs == 1);
	full = 0;
	close(0);
	close(2);
	int g = xnudrm_dupfd_cloexec(f), flags = fcntl(g, F_GETFD), low = fcntl(0, F_GETFD) < 0 && fcntl(2, F_GETFD) < 0;
	dup2(err, 2);
	CHECK(g >= 3 && (flags & FD_CLOEXEC) && low && wait_fd(g, 0) == 0);
}
static void t_denials(void)	/* a denied call is not tried again (and logged) for every fence */
{
	int n = node();
	for (int i = 0; i < 100; i++) xnudrm_close(submit(n));
	CHECK(denials <= 3);
}

int main(void)
{
	int fd;
	while ((fd = open("/dev/null", O_RDWR)) >= 0 && fd < 3) ;	/* the tests count on descriptors 0-2 being taken */
	if (fd > 2) close(fd);
	static const struct { const char *name; void (*f)(void); } tests[] = {
		{ "signal", t_signal }, { "close", t_close }, { "unwaited", t_unwaited }, { "node close", t_node_close }, { "stale fd, new", t_stale_new },
		{ "stale fd, dup", t_stale_dup }, { "multi", t_multi }, { "mixed", t_mixed },
		{ "error", t_error }, { "full", t_full }, { "dup flags", t_dup_flags }, { "denials", t_denials },
	};
	int bad = 0;
	for (int s = 0; s < 2; s++) {
		for (unsigned i = 0; i < sizeof(tests) / sizeof(tests[0]); i++) {
			int st, r;
			fflush(stdout);
			pid_t pid = fork();
			if (pid < 0) { perror("fork"); return 1; }
			if (!pid) { sandbox = s; alarm(10); tests[i].f(); _exit(0); }
			do r = waitpid(pid, &st, 0); while (r < 0 && errno == EINTR);
			if (r != pid) { perror("waitpid"); return 1; }
			printf("%s%s: %s\n", tests[i].name, s ? " (sandbox)" : "", !st ? "ok" : WIFSIGNALED(st) ? strsignal(WTERMSIG(st)) : "FAIL");
			bad |= st;
		}
	}
	return !!bad;
}

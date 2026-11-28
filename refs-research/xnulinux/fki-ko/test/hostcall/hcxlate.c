// SPDX-License-Identifier: GPL-2.0-only
/*
 * The ioctls of /dev/xnu-hostcall (fki_hostcall.c), as a service uses them for memory it shares with the hosted kernel:
 * XNU_HOSTCALL_XLATE over many pages against the kernel's own answers (/proc/self/pagemap), the errors it states, and
 * XNU_HOSTCALL_PIN / UNPIN. One check is about what else the process does meanwhile: XLATE writing into an output array
 * that is not in memory yet while another thread maps and unmaps memory, which is what a service with more than one
 * thread does (xnudrmd has the virglrenderer's threads beside it).
 * Needs root and fki.ko loaded; no hosted system. Exit status 0: all as stated; 1: not; 2: could not run.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

struct xnu_hostcall_xlate { uint64_t va, len, out, max; };
#define XNU_HOSTCALL_XLATE _IOWR('X', 1, struct xnu_hostcall_xlate)
struct xnu_hostcall_range { uint64_t va, len; };
#define XNU_HOSTCALL_PIN _IOW('X', 2, struct xnu_hostcall_range)
#define XNU_HOSTCALL_UNPIN _IOW('X', 3, struct xnu_hostcall_range)

static int hc, bad;

static int xlate(void *va, uint64_t len, uint64_t *out, uint64_t max)
{
	struct xnu_hostcall_xlate x = { (uintptr_t)va, len, (uintptr_t)out, max };
	return ioctl(hc, XNU_HOSTCALL_XLATE, &x) ? -errno : 0;
}

static int range(unsigned long cmd, void *va, uint64_t len)
{
	struct xnu_hostcall_range r = { (uintptr_t)va, len };
	return ioctl(hc, cmd, &r) ? -errno : 0;
}

static void is(const char *what, int got, int want)
{
	printf("%s: %s: %s", got == want ? "ok" : "FAIL", what, got ? strerror(-got) : "0");
	if (got != want) { printf(" (to be %s)", want ? strerror(-want) : "0"); bad = 1; }
	putchar('\n');
}

static void *anon(size_t len, int prot)
{
	void *p = mmap(NULL, len, prot, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	if (p == MAP_FAILED) { perror("mmap"); exit(2); }
	return p;
}

/* the first mapping of /proc/self/maps with this name ([vvar]: a mapping without pages of its own, as a device's is) */
static void *named(const char *name)
{
	char line[512];
	unsigned long a = 0;
	FILE *f = fopen("/proc/self/maps", "r");

	while (f && fgets(line, sizeof(line), f))
		if (strstr(line, name)) { sscanf(line, "%lx", &a); break; }
	if (f) fclose(f);
	return (void *)a;
}

static volatile int stop;
static void *mapper(void *arg)
{
	while (!stop) {
		void *p = mmap(NULL, 4096, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
		if (p != MAP_FAILED) munmap(p, 4096);
	}
	return arg;
}

/* In a child (a call that never returns can only be killed): rounds of XLATE into an output array that is not in memory. */
static void while_mapping(uint8_t *mem, size_t pages)
{
	enum { ROUNDS = 200 };
	int pfd[2], st, done = 0;
	pid_t pid;
	char c;

	if (pipe(pfd)) { perror("pipe"); exit(2); }
	pid = fork();
	if (pid < 0) { perror("fork"); exit(2); }
	if (!pid) {
		uint64_t *out = anon(pages * 8, PROT_READ | PROT_WRITE);
		pthread_t th;
		int i;

		close(pfd[0]);
		if (pthread_create(&th, NULL, mapper, NULL)) _exit(2);
		for (i = 0; i < ROUNDS; i++) {
			madvise(out, pages * 8, MADV_DONTNEED);		/* not in memory: the kernel's write faults it in */
			if (xlate(mem, pages * 4096, out, pages)) _exit(3);
			if (write(pfd[1], "", 1) != 1) _exit(2);
		}
		_exit(0);
	}
	close(pfd[1]);
	alarm(20);		/* a round takes milliseconds; SIGALRM ends the read below */
	while (read(pfd[0], &c, 1) == 1) done++;
	alarm(0);
	if (done < ROUNDS) kill(pid, SIGKILL);
	waitpid(pid, &st, 0);
	if (done == ROUNDS && WIFEXITED(st) && !WEXITSTATUS(st)) {
		printf("ok: XLATE into an output array not in memory, another thread mapping and unmapping: %d calls returned\n", done);
	} else if (WIFSIGNALED(st)) {
		printf("FAIL: XLATE into an output array not in memory, another thread mapping and unmapping: call %d and the other thread's mmap() did not return in 20 s (the process had to be killed)\n", done + 1);
		bad = 1;
	} else {
		printf("FAIL: XLATE into an output array not in memory: call %d failed\n", done + 1);
		bad = 1;
	}
}

static void on_alarm(int sig) { (void)sig; }

int main(void)
{
	enum { PAGES = 4096 };		/* 16 MiB: an output array of 8 pages */
	struct sigaction sa = { .sa_handler = on_alarm };
	uint64_t *out, e;
	uint8_t *mem, *vvar;
	size_t i, wrong = 0;
	int pm;

	sigaction(SIGALRM, &sa, NULL);		/* no SA_RESTART: the alarm ends a read() */
	hc = open("/dev/xnu-hostcall", O_RDWR | O_CLOEXEC);
	if (hc < 0) { perror("/dev/xnu-hostcall (is fki.ko loaded, are you root?)"); return 2; }
	pm = open("/proc/self/pagemap", O_RDONLY);
	if (pm < 0) { perror("/proc/self/pagemap"); return 2; }
	mem = anon(PAGES * 4096, PROT_READ | PROT_WRITE);
	out = anon(PAGES * 8, PROT_READ | PROT_WRITE);
	if (mlock(mem, PAGES * 4096)) { perror("mlock"); return 2; }	/* in memory, and not moved while we compare */

	is("XLATE of 4096 pages", xlate(mem, PAGES * 4096, out, PAGES), 0);
	for (i = 0; i < PAGES; i++) {
		if (pread(pm, &e, 8, ((uintptr_t)mem + i * 4096) / 4096 * 8) != 8 || !(e >> 63)) { fprintf(stderr, "pagemap has no entry for page %zu\n", i); return 2; }
		if (out[i] != (e & ((1ULL << 55) - 1)) * 4096) wrong++;
	}
	printf("%s: its answers against /proc/self/pagemap: %zu of %d differ\n", wrong ? "FAIL" : "ok", wrong, PAGES);
	if (wrong) bad = 1;

	is("XLATE of an address that is not page aligned", xlate(mem + 8, 4096, out, 1), -EINVAL);
	is("XLATE of a length that is not whole pages", xlate(mem, 100, out, 1), -EINVAL);
	is("XLATE of 2 pages with room for 1 answer", xlate(mem, 8192, out, 1), -E2BIG);
	munmap(mem + 4096, 4096);
	is("XLATE of 3 pages, the second not mapped", xlate(mem, 3 * 4096, out, 3), -EFAULT);
	is("XLATE with no output array", xlate(mem, 4096, NULL, 1), -EFAULT);

	is("PIN of 16 pages", range(XNU_HOSTCALL_PIN, mem + 8192, 16 * 4096), 0);
	is("UNPIN of them", range(XNU_HOSTCALL_UNPIN, mem + 8192, 0), 0);
	is("UNPIN again", range(XNU_HOSTCALL_UNPIN, mem + 8192, 0), -ENOENT);
	is("PIN of no pages", range(XNU_HOSTCALL_PIN, mem + 8192, 0), -EINVAL);
	is("PIN of an address that is not page aligned", range(XNU_HOSTCALL_PIN, mem + 8192 + 8, 4096), -EINVAL);
	is("PIN of a range with a page not mapped", range(XNU_HOSTCALL_PIN, mem, 3 * 4096), -EFAULT);
	vvar = named("[vvar");
	if (vvar)
		is("PIN of a mapping without pages of its own ([vvar])", range(XNU_HOSTCALL_PIN, vvar, 4096), -EOPNOTSUPP);
	else
		puts("skipped: PIN of a mapping without pages of its own (this process has no [vvar])");
	is("PIN of 4 pages, left to the close", range(XNU_HOSTCALL_PIN, mem + 8192, 4 * 4096), 0);

	while_mapping(mem + 8192, PAGES - 2);
	close(hc);
	return bad;
}

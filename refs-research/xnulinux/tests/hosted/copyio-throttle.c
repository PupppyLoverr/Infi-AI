// SPDX-License-Identifier: GPL-2.0-only
/*
 * A page-in taken inside copyin/copyout does not sleep the thread's I/O throttle window there, with the system call's
 * locks held (xnu-patches/0017). A program for the hosted system (PureDarwin's userland or macOS; no privileges):
 *
 *   cc -O2 -mmacosx-version-min=11.0 copyio-throttle.c -o copyio-throttle      (on a Mac; put it on the hosted disk)
 *   copyio-throttle DIR [SECONDS (10)]         in the hosted shell; DIR is on the disk to test, two 64 MiB files are
 *                                              made there and removed. Exit status 1: the pipe was held.
 *
 *   writer thread  in the throttled disk I/O tier (setiopolicy_np, thread scope). It write()s a file-backed mapping
 *                  into a pipe, a page that is not in memory each time, so the kernel's copyin takes a page-in from
 *                  disk inside write(), with the pipe locked (pipe_write holds the pipe's I/O lock across uiomove).
 *   noise thread   normal tier: reads another file from the same disk, uncached, so the throttle has something to
 *                  give way to.
 *   main thread    normal tier: every 2 ms one non-blocking read() from the pipe, timed. A read of a pipe whose I/O
 *                  lock is held waits for the lock, O_NONBLOCK or not.
 *
 * XNU marks a fault taken in copyin/copyout so that vm_fault does not sleep the thread's I/O throttle window there;
 * the thread sleeps it on the way out of the system call, holding nothing. Then every read here takes microseconds
 * however long the writer's write()s take. If the kernel sleeps the window inside copyin (the hosted kernel's own
 * fault paths before 0017), reads wait for it: the "reads over 20 ms" line counts them. (A read also waits for a
 * page-in that is in flight, in any kernel: milliseconds, which is why the line is drawn at 20.)
 *
 * The throttle must be on for the disk, or nothing is tested and the last line says so: XNU switches it off for a
 * disk that schedules I/O tiers itself (a Mac's own), and keeps it for the hosted disk, with the rotating-disk
 * numbers (one throttled I/O per 200 ms).
 */
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define FILE_BYTES      (64u << 20)
#define CHUNK           4096u
#define SLOW_US         20000u          /* above any one page-in, at or below one throttle period (25 ms on an SSD) */

static volatile int stop;
static int pipe_r, pipe_w, noise_fd;
static const char *map;
static size_t pagesz;
static unsigned long writes, write_max_us, write_total_us, write_slow;
static int policy_rc = -1, policy_errno;

static uint64_t
now_us(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (uint64_t)ts.tv_sec * 1000000u + (uint64_t)ts.tv_nsec / 1000u;
}

static void *
writer(void *arg)
{
	size_t npages = FILE_BYTES / pagesz, page = 0;

	(void)arg;
	policy_rc = setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD, IOPOL_THROTTLE);
	policy_errno = errno;
	while (!stop && page < npages) {
		uint64_t t0 = now_us();
		ssize_t n = write(pipe_w, map + page * pagesz, CHUNK);  /* copyin of a page that is still on disk */
		uint64_t d = now_us() - t0;

		if (n < 0 && errno != EINTR) {
			break;
		}
		writes++;
		write_total_us += d;
		if (d > write_max_us) {
			write_max_us = d;
		}
		if (d > SLOW_US) {
			write_slow++;
		}
		page += 3;      /* never the next page: no read-ahead does this one's work */
	}
	return NULL;
}

static void *
noise(void *arg)
{
	static char buf[256 << 10];
	off_t off = 0;

	(void)arg;
	while (!stop) {
		if (pread(noise_fd, buf, sizeof(buf), off) <= 0) {
			off = 0;
			continue;
		}
		off += sizeof(buf);
		if (off >= (off_t)FILE_BYTES) {
			off = 0;
		}
		usleep(1000);
	}
	return NULL;
}

static int
make_file(const char *path)
{
	static char block[1 << 20];
	int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0600);

	if (fd < 0) {
		return -1;
	}
	fcntl(fd, F_NOCACHE, 1);        /* written past the cache: its pages are on disk only */
	for (unsigned i = 0; i < sizeof(block); i++) {
		block[i] = (char)(i * 131u + 7u);
	}
	for (unsigned done = 0; done < FILE_BYTES; done += sizeof(block)) {
		if (write(fd, block, sizeof(block)) != (ssize_t)sizeof(block)) {
			close(fd);
			return -1;
		}
	}
	fsync(fd);
	return fd;
}

static int
cmp_u32(const void *a, const void *b)
{
	uint32_t x = *(const uint32_t *)a, y = *(const uint32_t *)b;

	return x < y ? -1 : x > y;
}

int
main(int argc, char **argv)
{
	const char *dir = argc > 1 ? argv[1] : ".";
	int seconds = argc > 2 ? atoi(argv[2]) : 10;
	char a[1024], b[1024];
	static char buf[64 << 10];      /* everything in the pipe at once: the writer never waits for room */
	int fds[2], map_fd;
	pthread_t tw, tn;
	uint32_t *lat;
	unsigned long nread = 0, got = 0, slow = 0, cap = (unsigned long)seconds * 600u + 16u;

	signal(SIGPIPE, SIG_IGN);
	pagesz = (size_t)sysconf(_SC_PAGESIZE);
	snprintf(a, sizeof(a), "%s/copyio-throttle.map.%d", dir, (int)getpid());
	snprintf(b, sizeof(b), "%s/copyio-throttle.noise.%d", dir, (int)getpid());
	map_fd = make_file(a);
	noise_fd = make_file(b);
	if (map_fd < 0 || noise_fd < 0) {
		perror("copyio-throttle: making the two files");
		unlink(a);
		unlink(b);
		return 2;
	}
	map = mmap(NULL, FILE_BYTES, PROT_READ, MAP_PRIVATE, map_fd, 0);
	if (map == MAP_FAILED || pipe(fds) != 0 || (lat = calloc(cap, sizeof(*lat))) == NULL) {
		perror("copyio-throttle: mmap/pipe");
		unlink(a);
		unlink(b);
		return 2;
	}
	madvise((void *)(uintptr_t)map, FILE_BYTES, MADV_RANDOM);       /* a fault brings in its own page only */
	pipe_r = fds[0];
	pipe_w = fds[1];
	fcntl(pipe_r, F_SETFL, O_NONBLOCK);
	pthread_create(&tn, NULL, noise, NULL);
	pthread_create(&tw, NULL, writer, NULL);

	uint64_t end = now_us() + (uint64_t)seconds * 1000000u;
	while (now_us() < end && nread < cap) {
		usleep(2000);
		uint64_t t0 = now_us();
		ssize_t n = read(pipe_r, buf, sizeof(buf));
		uint64_t d = now_us() - t0;

		lat[nread++] = d > 0xffffffffu ? 0xffffffffu : (uint32_t)d;
		got += n > 0;
		slow += d > SLOW_US;
	}
	stop = 1;
	close(pipe_r);                  /* a writer waiting for room in the pipe comes back with EPIPE */
	pthread_join(tw, NULL);
	pthread_join(tn, NULL);
	unlink(a);
	unlink(b);

	qsort(lat, nread, sizeof(*lat), cmp_u32);
	printf("copyio-throttle: %d s on %s, page size %zu; writer's I/O tier: %s\n", seconds, dir, pagesz,
	    policy_rc == 0 ? "throttled (setiopolicy_np ok)" : strerror(policy_errno));
	printf("copyio-throttle: writer  %lu write()s of %u bytes, each from a page still on disk: longest %lu us, mean %lu us, over 20 ms %lu\n",
	    writes, CHUNK, write_max_us, writes ? write_total_us / writes : 0, write_slow);
	printf("copyio-throttle: reader  %lu read()s (%lu with data): longest %u us, 99%% %u us, median %u us\n",
	    nread, got, nread ? lat[nread - 1] : 0, nread ? lat[nread * 99 / 100] : 0, nread ? lat[nread / 2] : 0);
	printf("copyio-throttle: reads over 20 ms: %lu%s\n", slow,
	    write_slow == 0 ? "   (the writer was never slowed: the throttle did not engage, nothing was tested)" :
	    slow ? "   <- the pipe was held while its writer slept" : "   (the writer slept, never with the pipe held)");
	return slow ? 1 : 0;
}

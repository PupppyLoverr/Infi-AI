// SPDX-License-Identifier: GPL-2.0-only
/*
 * A service on /dev/xnu-hostcall for the tests (run.sh): what gpu/xnu-drm/xnudrmd is to the hosted kernel, with one way
 * of going wrong per mode. Its reply to a request is the request's first 8 bytes (the caller's tag) and the rest with
 * every byte inverted, so the caller (hctest.c) can tell the reply to its own request from any other.
 *   echo            read, reply
 *   smallbuf        the first read() of a request has a buffer too small for it; then as echo
 *   readfault       the first read() of a request faults in the middle of the buffer; then as echo
 *   writefault      the first write() of a reply faults in the middle of the reply; then the reply again, whole
 *   close           read one request and exit without a reply
 *   bigreply LEN    answer with LEN bytes whatever the request
 *   noreply SECS    read one request, write its reply SECS later (the call has timed out: -ESTALE), then as echo
 *   slowwrite SECS  the first reply's bytes are in memory that is not there until SECS have passed (userfaultfd): the
 *                   write() is inside the kernel all that time; then as echo
 * Prints "ready" once the device is open, then one line per system call that matters.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/userfaultfd.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <unistd.h>

#define MAX ((8u << 20) + 4096)		/* more than any request: sizeof(struct xnudrm_hdr) + XNUDRM_MAX_MSG is the bridge's */

static int hc, uffd;
static unsigned secs;
static uint8_t *two;			/* two pages; the second is taken away or not there yet, as the mode wants */
static uint8_t later[4096];		/* slowwrite: what the second page will hold */

static void say(const char *what, ssize_t r)
{
	if (r < 0) printf("%s = -1 (%s)\n", what, strerror(errno)); else printf("%s = %zd\n", what, r);
	fflush(stdout);
}

static void *resolve(void *arg)
{
	struct uffd_msg m;
	struct uffdio_copy c = { .dst = (uintptr_t)two + 4096, .src = (uintptr_t)later, .len = 4096 };

	if (read(uffd, &m, sizeof(m)) != sizeof(m)) { perror("userfaultfd read"); exit(2); }
	printf("slowwrite: write() is in the kernel and waits for the reply's memory; it gets it in %u s\n", secs); fflush(stdout);
	sleep(secs);
	if (ioctl(uffd, UFFDIO_COPY, &c)) { perror("UFFDIO_COPY"); exit(2); }
	return arg;
}

int main(int argc, char **argv)
{
	const char *mode = argc > 1 ? argv[1] : "echo";
	size_t biglen = argc > 2 ? strtoul(argv[2], NULL, 0) : 0;
	uint8_t *msg = malloc(8 + MAX), *rep = malloc(8 + MAX);
	int first = 1;

	secs = (unsigned)biglen;
	two = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	if (!msg || !rep || two == MAP_FAILED) return 2;
	hc = open("/dev/xnu-hostcall", O_RDWR | O_CLOEXEC);
	if (hc < 0) { perror("/dev/xnu-hostcall (is fki.ko loaded, are you root?)"); return 2; }
	if (!strcmp(mode, "slowwrite")) {
		struct uffdio_api api = { .api = UFFD_API };
		struct uffdio_register reg = { .range = { (uintptr_t)two + 4096, 4096 }, .mode = UFFDIO_REGISTER_MODE_MISSING };
		pthread_t th;

		uffd = syscall(SYS_userfaultfd, O_CLOEXEC);
		if (uffd < 0 || ioctl(uffd, UFFDIO_API, &api) || ioctl(uffd, UFFDIO_REGISTER, &reg)) { perror("userfaultfd"); return 2; }
		if (pthread_create(&th, NULL, resolve, NULL)) return 2;
	} else if (!strcmp(mode, "readfault") || !strcmp(mode, "writefault")) {
		two[0] = 0;
		if (mprotect(two + 4096, 4096, PROT_NONE)) return 2;
	}
	puts("ready"); fflush(stdout);
	for (;;) {
		ssize_t n, w;
		size_t i, rn;

		if (first && !strcmp(mode, "smallbuf")) {
			n = read(hc, msg, 8);
			say("read() with room for 8 bytes", n);
		}
		if (first && !strcmp(mode, "readfault")) {
			n = read(hc, two + 4096 - 16, 8 + MAX);
			say("read() into a buffer whose 17th byte is not mapped", n);
		}
		n = read(hc, msg, 8 + MAX);
		if (n < 0 && errno == EINTR) continue;
		say("read()", n);
		if (n < 16) return 1;
		if (first && !strcmp(mode, "close")) { puts("exit without a reply"); return 0; }
		rn = !strcmp(mode, "bigreply") ? 8 + biglen : (size_t)n;
		memcpy(rep, msg, 16);
		for (i = 16; i < rn; i++) rep[i] = i < (size_t)n ? (uint8_t)~msg[i] : (uint8_t)i;
		if (first && !strcmp(mode, "noreply")) {
			printf("no reply for %u s\n", secs); fflush(stdout);
			sleep(secs);
			w = write(hc, rep, rn);
			say("the late write()", w);
			first = 0;
			continue;
		}
		if (first && !strcmp(mode, "writefault")) {
			memcpy(two + 4096 - 16, rep, 16);
			w = write(hc, two + 4096 - 16, rn);
			say("write() of a reply whose 17th byte is not mapped", w);
		}
		if (first && !strcmp(mode, "slowwrite")) {
			memcpy(two + 4096 - 8, rep, 8);		/* the sequence number; the reply itself is on the page to come */
			memcpy(later, rep + 8, rn - 8);
			w = write(hc, two + 4096 - 8, rn);
			say("the slow write()", w);
			first = 0;
			continue;
		}
		w = write(hc, rep, rn);
		say("write()", w);
		first = 0;
	}
}

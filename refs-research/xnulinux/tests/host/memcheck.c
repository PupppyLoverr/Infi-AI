// SPDX-License-Identifier: GPL-2.0-only
/* Fill GBs of memory with a pattern derived from each word's address, re-verify it in a loop and report every word
 * that changed: its virtual and physical address (/proc/self/pagemap, root) and the value found. A probe for a foreign
 * writer (the hosted kernel, a device) scribbling over Linux's memory. Usage: memcheck GB SECONDS */
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

static uint64_t pat(uint64_t *p) { return ((uint64_t)p * 0x9E3779B97F4A7C15ULL) ^ 0x5A5A5A5AA5A5A5A5ULL; }

static uint64_t phys(int pm, void *va)
{
	uint64_t e = 0;
	if (pread(pm, &e, 8, ((uintptr_t)va / 4096) * 8) != 8 || !(e >> 63)) return 0;
	return ((e & ((1ULL << 55) - 1)) << 12) | ((uintptr_t)va & 4095);
}

int main(int argc, char **argv)
{
	size_t gb = argc > 1 ? atol(argv[1]) : 2, n = gb << 27;	/* words */
	int secs = argc > 2 ? atoi(argv[2]) : 120, pm = open("/proc/self/pagemap", O_RDONLY), bad = 0, rounds = 0;
	uint64_t *m = mmap(NULL, n * 8, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_POPULATE, -1, 0);
	if (m == MAP_FAILED) { perror("mmap"); return 1; }
	mlock(m, n * 8);
	for (size_t i = 0; i < n; i++) m[i] = pat(&m[i]);
	time_t end = time(NULL) + secs;
	while (time(NULL) < end) {
		for (size_t i = 0; i < n; i++) {
			uint64_t want = pat(&m[i]);
			if (m[i] != want) {
				if (bad++ < 64)
					printf("changed: va %p pa %#llx found %#018llx want %#018llx\n", (void *)&m[i],
					    (unsigned long long)phys(pm, &m[i]), (unsigned long long)m[i], (unsigned long long)want);
				m[i] = want;
			}
		}
		rounds++;
	}
	printf("%zu GB, %d rounds, %d words changed\n", gb, rounds, bad);
	return bad != 0;
}

// SPDX-License-Identifier: GPL-2.0-only
/* Keep known values in the FP/SIMD registers v8..v15 on one CPU while sleeping in a loop (so the task is switched out
 * and back in), and count the times they came back changed: a probe for a foreign kernel using SIMD on that CPU
 * behind Linux's back (Linux skips reloading a task's FP state when it believes the registers still hold it).
 * Usage: fpcheck CPU SECONDS */
#define _GNU_SOURCE
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

int main(int argc, char **argv)
{
	int cpu = argc > 1 ? atoi(argv[1]) : 0, secs = argc > 2 ? atoi(argv[2]) : 60;
	cpu_set_t s; CPU_ZERO(&s); CPU_SET(cpu, &s);
	if (sched_setaffinity(0, sizeof(s), &s)) { perror("affinity"); return 1; }
	uint64_t want = 0x0123456789abcdefULL, got[8];
	long rounds = 0, bad = 0;
	time_t end = time(NULL) + secs;
	while (time(NULL) < end) {
		__asm__ volatile("dup v8.2d, %0\n dup v9.2d, %0\n dup v10.2d, %0\n dup v11.2d, %0\n"
		                 "dup v12.2d, %0\n dup v13.2d, %0\n dup v14.2d, %0\n dup v15.2d, %0" :: "r"(want) : "v8","v9","v10","v11","v12","v13","v14","v15");
		for (int i = 0; i < 20; i++) sched_yield();	/* switched out when anything else on this CPU wants to run */
		usleep(200);
		__asm__ volatile("umov %0, v8.d[0]\n umov %1, v9.d[0]\n umov %2, v10.d[0]\n umov %3, v11.d[0]\n"
		                 "umov %4, v12.d[0]\n umov %5, v13.d[0]\n umov %6, v14.d[0]\n umov %7, v15.d[0]"
		                 : "=r"(got[0]), "=r"(got[1]), "=r"(got[2]), "=r"(got[3]), "=r"(got[4]), "=r"(got[5]), "=r"(got[6]), "=r"(got[7]));
		for (int i = 0; i < 8; i++) if (got[i] != want) { if (bad++ < 10) printf("v%d changed: %#018llx\n", 8 + i, (unsigned long long)got[i]); break; }
		rounds++; want = want * 6364136223846793005ULL + 1442695040888963407ULL;
	}
	printf("cpu %d: %ld rounds, %ld with SIMD registers changed\n", cpu, rounds, bad);
	return bad != 0;
}

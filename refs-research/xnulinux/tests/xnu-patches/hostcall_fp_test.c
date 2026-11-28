// SPDX-License-Identifier: APSL-2.0
/*
 * A call into the host does not keep the FP/SIMD registers: Linux schedules inside it (idle, the preempted call, disk
 * and network I/O) and a kernel thread's v0-v31 are neither saved nor put back. Code around linux64_call2 must
 * therefore come out right when the hook returns with every one of them changed, d8-d15 included.
 *
 * Built against xnu-patches/include only and run as an ordinary program on an arm64 machine: the hook below stands
 * in for the host and overwrites v0-v31.
 */
#include <stdio.h>
#include <string.h>
#include <linux64/linux64.h>

unsigned long linux64_hook;
unsigned long linux64_dbg[LINUX64_NCOUNTERS];
unsigned int linux64_el0;

static volatile unsigned long hook_calls;

/* The host: answers 0 and leaves all 32 FP/SIMD registers changed (nothing is declared: the compiler saves none). */
__attribute__((noinline, used)) static unsigned long
host_hook(unsigned long call, unsigned long a1, unsigned long a2)
{
	(void)call; (void)a1; (void)a2;
	hook_calls++;
	__asm__ volatile (
		"movi v0.16b, #0xff\n movi v1.16b, #0xff\n movi v2.16b, #0xff\n movi v3.16b, #0xff\n"
		"movi v4.16b, #0xff\n movi v5.16b, #0xff\n movi v6.16b, #0xff\n movi v7.16b, #0xff\n"
		"movi v8.16b, #0xff\n movi v9.16b, #0xff\n movi v10.16b, #0xff\n movi v11.16b, #0xff\n"
		"movi v12.16b, #0xff\n movi v13.16b, #0xff\n movi v14.16b, #0xff\n movi v15.16b, #0xff\n"
		"movi v16.16b, #0xff\n movi v17.16b, #0xff\n movi v18.16b, #0xff\n movi v19.16b, #0xff\n"
		"movi v20.16b, #0xff\n movi v21.16b, #0xff\n movi v22.16b, #0xff\n movi v23.16b, #0xff\n"
		"movi v24.16b, #0xff\n movi v25.16b, #0xff\n movi v26.16b, #0xff\n movi v27.16b, #0xff\n"
		"movi v28.16b, #0xff\n movi v29.16b, #0xff\n movi v30.16b, #0xff\n movi v31.16b, #0xff\n");
	return 0;
}

struct s { long a[8]; };

/* a structure zeroed before the call and another after it: the compiler may keep one zero vector for both */
__attribute__((noinline)) static void
zero_twice(struct s *p, struct s *q)
{
	*p = (struct s){ 0 };
	linux64_call2(LINUX64_CALL_PREEMPTED, 1, 0);
	*q = (struct s){ 0 };
}

/* a floating-point value used after the call, in the function that makes it */
__attribute__((noinline)) static double
fp_across(double x)
{
	linux64_call2(LINUX64_CALL_PREEMPTED, 1, 0);
	return x + x;
}

/* a function that only makes the call, and a caller that keeps a double of its own across it (in d8-d15) */
__attribute__((noinline)) static void
leaf(void)
{
	linux64_call2(LINUX64_CALL_PREEMPTED, 1, 0);
}

__attribute__((noinline)) static double
fp_caller(double x)
{
	leaf();
	return x + x;
}

int
main(void)
{
	static volatile double in = 21.0;
	struct s p, q;
	int bad = 0;

	linux64_hook = (unsigned long)&host_hook;

	memset(&p, 0x55, sizeof(p));
	memset(&q, 0x55, sizeof(q));
	zero_twice(&p, &q);
	int nz = 0;
	for (int i = 0; i < 8; i++) {
		nz += p.a[i] != 0 || q.a[i] != 0;
	}
	printf("%s  a structure zeroed after a host call is all zero\n", nz ? "FAIL" : "ok  ");
	bad += nz != 0;

	double r = fp_across(in);
	printf("%s  a double used after a host call in the same function: 21 + 21 = %g\n", r == 42.0 ? "ok  " : "FAIL", r);
	bad += r != 42.0;

	r = fp_caller(in);
	printf("%s  a double a caller keeps across a function that makes a host call: 21 + 21 = %g\n",
	    r == 42.0 ? "ok  " : "FAIL", r);
	bad += r != 42.0;

	if (hook_calls != 3) {
		printf("FAIL  the hook ran %lu times, not 3\n", (unsigned long)hook_calls);
		bad++;
	}
	printf("%d of 3 checks failed\n", bad > 3 ? 3 : bad);
	return bad != 0;
}

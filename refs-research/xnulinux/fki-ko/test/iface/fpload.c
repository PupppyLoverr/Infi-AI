// SPDX-License-Identifier: GPL-2.0-only
/*
 * fki_enter() gives the CPU's FP/SIMD registers to the foreign kernel, "the current task's state saved first" (fki.h).
 * This is the current task: it loads ifacetest.ko (t=fp, whose init writes every FP/SIMD register inside fki_enter())
 * with all 32 registers, FPSR and FPCR holding known values, and looks at them when the system call is back.
 *   fpload ifacetest.ko        exit status 0: all as they were; 1: not; 2: could not run
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

static uint8_t in[32][16], out[32][16];

int main(int argc, char **argv)
{
	int fd = open(argc > 1 ? argv[1] : "ifacetest.ko", O_RDONLY | O_CLOEXEC), i, j, changed = 0;
	uint64_t fpsr = 0x1f, fpcr = 0xc00000, fpsr2 = 0, fpcr2 = 0;	/* every cumulative flag; round towards zero */
	register long x0 __asm__("x0") = fd;
	register const char *x1 __asm__("x1") = "t=fp";
	register long x2 __asm__("x2") = 0;
	register long x8 __asm__("x8") = SYS_finit_module;

	if (fd < 0) { perror(argc > 1 ? argv[1] : "ifacetest.ko"); return 2; }
	for (i = 0; i < 32; i++) memset(in[i], 0x40 + i, 16);
	__asm__ volatile(
	    "msr fpsr, %[fpsr]\n msr fpcr, %[fpcr]\n"
	    "ldp q0, q1, [%[in], #0]\n ldp q2, q3, [%[in], #32]\n ldp q4, q5, [%[in], #64]\n ldp q6, q7, [%[in], #96]\n"
	    "ldp q8, q9, [%[in], #128]\n ldp q10, q11, [%[in], #160]\n ldp q12, q13, [%[in], #192]\n ldp q14, q15, [%[in], #224]\n"
	    "ldp q16, q17, [%[in], #256]\n ldp q18, q19, [%[in], #288]\n ldp q20, q21, [%[in], #320]\n ldp q22, q23, [%[in], #352]\n"
	    "ldp q24, q25, [%[in], #384]\n ldp q26, q27, [%[in], #416]\n ldp q28, q29, [%[in], #448]\n ldp q30, q31, [%[in], #480]\n"
	    "svc #0\n"
	    "stp q0, q1, [%[out], #0]\n stp q2, q3, [%[out], #32]\n stp q4, q5, [%[out], #64]\n stp q6, q7, [%[out], #96]\n"
	    "stp q8, q9, [%[out], #128]\n stp q10, q11, [%[out], #160]\n stp q12, q13, [%[out], #192]\n stp q14, q15, [%[out], #224]\n"
	    "stp q16, q17, [%[out], #256]\n stp q18, q19, [%[out], #288]\n stp q20, q21, [%[out], #320]\n stp q22, q23, [%[out], #352]\n"
	    "stp q24, q25, [%[out], #384]\n stp q26, q27, [%[out], #416]\n stp q28, q29, [%[out], #448]\n stp q30, q31, [%[out], #480]\n"
	    "mrs %[fpsr2], fpsr\n mrs %[fpcr2], fpcr\n msr fpcr, xzr\n"
	    : "+r"(x0), [fpsr2] "=&r"(fpsr2), [fpcr2] "=&r"(fpcr2)
	    : "r"(x1), "r"(x2), "r"(x8), [in] "r"(in), [out] "r"(out), [fpsr] "r"(fpsr), [fpcr] "r"(fpcr)
	    : "memory", "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15", "v16",
	      "v17", "v18", "v19", "v20", "v21", "v22", "v23", "v24", "v25", "v26", "v27", "v28", "v29", "v30", "v31");
	if (x0 != -EAGAIN) { fprintf(stderr, "finit_module: %s (the module's init answers EAGAIN when it ran)\n", strerror((int)-x0)); return 2; }
	for (i = 0; i < 32; i++) {
		for (j = 0; j < 16 && out[i][j] == 0x40 + i; j++) ;
		if (j < 16) changed++;
	}
	printf("%s: this process's registers after the foreign kernel used the CPU's: %d of 32 FP/SIMD registers changed (v0 = %02x..., v31 = %02x...; set to 40, 5f), FPSR %#llx (set to 0x1f), FPCR %#llx (set to 0xc00000)\n",
	    changed || fpsr2 != fpsr || fpcr2 != fpcr ? "FAIL" : "ok", changed, out[0][0], out[31][0], (unsigned long long)fpsr2, (unsigned long long)fpcr2);
	return changed || fpsr2 != fpsr || fpcr2 != fpcr;
}

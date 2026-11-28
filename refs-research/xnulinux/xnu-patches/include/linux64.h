// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/*
 * The LINUX64 machine layer: XNU hosted inside a Linux kernel by xnu.ko. Everything the rest of XNU uses of it.
 * Copied with linux64_abi.h into EXTERNAL_HEADERS/linux64 (on every component's include path) when the patches are
 * applied (scripts/vm-xnu-rebuild.sh).
 *
 * Hosted or not is decided at run time: linux64_hook is 0 when this kernel boots on hardware, and every hosted path
 * is behind `if (linux64_hook)`.
 */
#ifndef _LINUX64_LINUX64_H_
#define _LINUX64_LINUX64_H_

#include <linux64/linux64_abi.h>

/* Bring-up diagnostics (thread dumps from the idle loop, per-exception traces): off unless built with -DLINUX64_DEBUG=1 */
#ifndef LINUX64_DEBUG
#define LINUX64_DEBUG 0
#endif

#ifdef __cplusplus
extern "C" {
#endif

/*
 * xnu.ko's entry point (0: not hosted), EL0-host flag (1: hosted kernel runs at EL0) and the slide it applied.
 * TPIDR_EL1 is Linux's per-CPU base (or inaccessible at EL0): XNU's kernel keeps its thread pointer in TPIDR_EL0
 * (per CPU), and user space gets its own TLS registers on each return to EL0.
 */
extern unsigned long linux64_hook;
extern unsigned long linux64_slide;
extern unsigned int linux64_el0;
extern unsigned int linux64_el0_caps;
/* This CPU's slot in the per-CPU arrays: valid before XNU knows its cpu number */
static inline unsigned
linux64_cpu_slot(void)
{
	if (linux64_el0) {
		unsigned long id;

		__asm__ volatile ("mrs %0, TPIDRRO_EL0" : "=r"(id));
		return (unsigned)(id & (LINUX64_CPU_SLOTS - 1));
	}
	unsigned long mpidr;

	__asm__ volatile ("mrs %0, MPIDR_EL1" : "=r"(mpidr));
	return LINUX64_CPU_SLOT(mpidr);
}

/* Each CPU's interrupt-enable state: hardware interrupts stay masked while XNU runs; this flag is the virtual one */
extern unsigned int linux64_vint[LINUX64_CPU_SLOTS];
#define LINUX64_VINT linux64_vint[linux64_cpu_slot()]
/* Each CPU's pending inter-processor interrupt (set by the sender, taken where XNU's interrupts are enabled again) */
extern unsigned int linux64_ipi_pending[LINUX64_CPU_SLOTS];
/* enum linux64_counter */
extern unsigned long linux64_dbg[LINUX64_NCOUNTERS];

/*
 * A call into the host (enum linux64_call). The host's answer: 0 to go on.
 *
 * The host keeps none of the FP/SIMD registers across it: it saves none of them, and where it lets Linux run (idle,
 * preempted, file I/O, the network, a host call) they come back as the last Linux task or kernel-mode NEON user left
 * them (Linux keeps no FP/SIMD state for a kernel thread). That includes d8-d15, which a callee owes its caller: the
 * function that makes the call saves them. Not inline, so that this is the one function, and only when a call is made
 * (inline, every function with a call in it saved them on every entry: ml_set_interrupts_enabled, each time).
 */
__attribute__((noinline, unused)) static unsigned long
linux64_call2(enum linux64_call call, unsigned long a1, unsigned long a2)
{
	register unsigned long x0 __asm__("x0") = (unsigned long)call;
	register unsigned long x1 __asm__("x1") = a1;
	register unsigned long x2 __asm__("x2") = a2;
	linux64_dbg[LINUX64_CTR_LAST_CALL] = (unsigned long)call;
	if (linux64_hook) {
		/* a plain blr: the hook is host code, not an XNU function pointer (no pointer authentication) */
		__asm__ volatile ("blr %3"
		    : "+r"(x0), "+r"(x1), "+r"(x2)
		    : "r"(linux64_hook)
		    : "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17",
		    "x30", "cc", "memory",
		    "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15",
		    "v16", "v17", "v18", "v19", "v20", "v21", "v22", "v23", "v24", "v25", "v26", "v27", "v28", "v29", "v30",
		    "v31");
	}
	return x0;
}

static inline unsigned long
linux64_call(enum linux64_call call, const void *a1)
{
	return linux64_call2(call, (unsigned long)a1, 0);
}

/* Bytes from the host's cryptographic generator (Linux's get_random_bytes) */
static inline void
linux64_random(void *buf, unsigned long len)
{
	linux64_call2(LINUX64_CALL_RANDOM, (unsigned long)buf, len);
}

/* A uniform value in [0, bound) (0 when bound is 0), by rejection: no modulo bias */
static inline __UINT64_TYPE__
linux64_random_uniform(__UINT64_TYPE__ bound)
{
	__UINT64_TYPE__ v, limit;

	if (bound == 0) {
		return 0;
	}
	limit = -bound % bound;	/* 2^64 mod bound: values below it would bias the result */
	do {
		linux64_random(&v, sizeof(v));
	} while (v < limit);
	return v % bound;
}

static inline void
linux64_count(enum linux64_counter c)
{
	linux64_dbg[c]++;
}

/* osfmk/arm/arm_init.c */
void linux64_assert_report(const char *file, int line, const char *expr);
/* osfmk/arm/machine_routines_common.c: IPIs. With XNU's interrupts off: take this CPU's pending IPI, if any */
void linux64_ipi_take(void);
/* signal a CPU (kexts/XnuCPU's IOCPU::signalCPU): its logical number */
void linux64_cpu_signal(unsigned int cpu);
/* bsd/kern/kern_trustcache.c, libkern/amfi/amfi.c */
void linux64_trust_cache_late_init(void);
int linux64_amfi_off(void);
/* osfmk/arm64/cswitch.s, sleh.c: user space while hosted */
void linux64_el0_run(void *saved_state, unsigned long arg) __attribute__((noreturn));
struct thread;
void linux64_el0_fpu_switch_out(struct thread *old);
void linux64_el0_fpu_flush(struct thread *thread);
void linux64_el0_fpu_invalidate(struct thread *thread);
/* osfmk/arm/machine_routines_common.c: in a spin-wait, let the host take this CPU now and then (see arm/locks.h) */
void linux64_spin_relax(void);
/* osfmk/kern/sched_prim.c: every thread's state and saved frames to the host log (on the host's request) */
void linux64_dump_threads(void);

#ifdef __cplusplus
}
#endif

#endif /* _LINUX64_LINUX64_H_ */

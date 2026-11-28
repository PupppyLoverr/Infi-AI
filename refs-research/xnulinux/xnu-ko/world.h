// SPDX-License-Identifier: APSL-2.0
/* World switch between Linux and the hosted XNU (see world.S for the layout). */
#pragma once
#include "../fki-ko/fki.h"
struct xnu_world {
	unsigned long long linux_regs[14];	/* 0: x19..x28, x29, x30, sp, irq cookie */
	unsigned long long xnu_regs[13];	/* 112: x19..x28, x29, x30, sp */
	unsigned long long linux_tpidr0;		/* 216: TPIDR_EL0 */
	unsigned long long linux_tpidrro0;	/* 224: TPIDRRO_EL0 */
	unsigned long long xnu_stack_top;	/* 232 */
	struct fki_pac_keys linux_keys;		/* 240 */
	struct fki_pac_keys xnu_keys;		/* 328 */
	unsigned long long linux_ttbr0, linux_tcr;	/* 416, 424: Linux's user address space while XNU runs */
	unsigned long long xnu_ttbr0, xnu_tcr;		/* 432, 440: XNU's (its user pmaps, its T0SZ) */
	unsigned long long xnu_tpidr, xnu_tpidrro;	/* 448, 456: XNU's user TLS registers across an upcall */
	unsigned long long linux_scs;		/* 464: x18, the shadow call stack of the Linux task that entered */
};
_Static_assert(__builtin_offsetof(struct xnu_world, linux_scs) == 464, "xnu_world layout");
_Static_assert(__builtin_offsetof(struct xnu_world, xnu_tpidrro) == 456, "xnu_world layout");
_Static_assert(__builtin_offsetof(struct xnu_world, linux_ttbr0) == 416, "xnu_world layout");
_Static_assert(sizeof(struct fki_pac_keys) == 88, "fki_pac_keys layout");
_Static_assert(__builtin_offsetof(struct xnu_world, xnu_keys) == 328, "xnu_world layout");
extern struct xnu_world xnu_W;		/* the boot CPU's */
extern struct xnu_world *xnu_worlds[];	/* each CPU's, by slot (LINUX64_CPU_SLOT) */
/* Run fn(arg) in the XNU world. 0: fn returned; 1: the hook ended the run. */
long xnu_world_call(unsigned long fn, unsigned long arg);
/* The address XNU calls (linux64_hook); it calls xnu_hook(stage) in the Linux world. */
void xnu_world_upcall(void);
/* Pure EL0 host variants (FKI_CAP_EL0): no EL1 system instructions; slot from TPIDRRO_EL0. */
long xnu_world_call_el0(unsigned long fn, unsigned long arg);
void xnu_world_upcall_el0(void);
/* Where an XNU fault that cannot be handled returns to (still with XNU's keys and user half): back to Linux's, then out. */
void xnu_world_fault_abort(void);
/* Linux-world handler: return 0 to resume XNU, nonzero to end the run. */
long xnu_hook(unsigned long stage, unsigned long a1, unsigned long a2);

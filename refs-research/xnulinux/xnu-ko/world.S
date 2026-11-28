// SPDX-License-Identifier: APSL-2.0
/* World switch, see world.h. No PAC instructions here: keys change under our feet.
 * x18 is Linux's shadow call stack pointer (CONFIG_SHADOW_CALL_STACK), which belongs to the Linux task that entered the
 * world on this CPU. XNU does not use it or save it per thread, so it stays this CPU's while XNU runs (Linux code XNU
 * calls pushes and pops on it, balanced); the way out puts back the value the task entered with.
 * TPIDR_EL1 stays Linux's: the LINUX64 XNU keeps its kernel thread pointer in TPIDR_EL0 (swapping user TPIDR_EL0 on
 * EL0 entry/exit and persisting it in thread->machine.linux64_user_tpidr), so a fault taken in XNU code reaches Linux
 * with a sane per-cpu base and becomes a normal oops. Each CPU has its own save area. */
/*
 * This CPU's save area: xnu_worlds[slot], slot = affinity 0 and 1 of MPIDR_EL1, four bits each (LINUX64_CPU_SLOT).
 * Clobbers x10.
 */
#define W(reg) mrs reg, mpidr_el1; and x10, reg, #0xf; ubfx reg, reg, #8, #4; orr reg, x10, reg, lsl #4; \
	adrp x10, xnu_worlds; add x10, x10, :lo12:xnu_worlds; ldr reg, [x10, reg, lsl #3]

/*
 * The user half (TTBR0, TCR.T0SZ) belongs to whichever world runs: XNU switches TTBR0 to its user pmaps (with its own
 * ASIDs and T0SZ), Linux to the current task's mm. Each switch saves one and loads the other, then drops the local TLB
 * (the two worlds' ASIDs are unrelated). x9 = &xnu_W; clobbers x10.
 */
.macro TO_XNU_MM
	mrs	x10, ttbr0_el1
	str	x10, [x9, #416]
	mrs	x10, tcr_el1
	str	x10, [x9, #424]
	ldr	x10, [x9, #440]
	msr	tcr_el1, x10
	ldr	x10, [x9, #432]
	msr	ttbr0_el1, x10
	isb
	tlbi	vmalle1
	dsb	nsh
	isb
.endm
.macro TO_LINUX_MM
	mrs	x10, ttbr0_el1
	str	x10, [x9, #432]
	mrs	x10, tcr_el1
	str	x10, [x9, #440]
	ldr	x10, [x9, #424]
	msr	tcr_el1, x10
	ldr	x10, [x9, #416]
	msr	ttbr0_el1, x10
	isb
	tlbi	vmalle1
	dsb	nsh
	isb
.endm

	.text
	.globl	xnu_world_call
	.type	xnu_world_call, %function
xnu_world_call:
	W(x9)
	str	x18, [x9, #464]
	stp	x19, x20, [x9, #0]
	stp	x21, x22, [x9, #16]
	stp	x23, x24, [x9, #32]
	stp	x25, x26, [x9, #48]
	stp	x27, x28, [x9, #64]
	stp	x29, x30, [x9, #80]
	mov	x10, sp
	str	x10, [x9, #96]
	mov	x19, x0
	mov	x20, x1
	add	x0, x9, #328		/* load XNU keys */
	add	x1, x9, #240		/* save Linux keys */
	bl	fki_enter		/* no preemption, no IRQs */
	W(x9)
	str	x0, [x9, #104]
	msr	daifset, #0xf		/* XNU's kernel runs with debug exceptions masked (PSTATE.D) */
	mrs	x10, tpidr_el0		/* XNU writes the user TLS registers of the thread it believes current */
	str	x10, [x9, #216]
	mrs	x10, tpidrro_el0
	str	x10, [x9, #224]
	TO_XNU_MM
	/* XNU runs on the Linux task stack: Linux's entry code treats any other stack as overflow. */
	mov	x0, x20
	blr	x19
	/* fn returned: XNU keys and thread pointer are live */
	W(x9)
	mov	x19, #0
	b	2f
/*
 * From the upcall: Linux keys, TPIDR and user half are live. The upcall's TO_LINUX_MM already switched, and Linux may
 * have scheduled during the host call since, so the TTBR0 saved at the last TO_XNU_MM is stale: loading it again would
 * leave this CPU on an mm Linux has moved off (the next task with the same mm faults on its user addresses for ever).
 */
xnu_world_abort:
	W(x9)
	mov	x19, #1
	b	3f
xnu_world_abort_mm:			/* from a fault in XNU: XNU's user half is live */
	W(x9)
	mov	x19, #1
2:	TO_LINUX_MM
3:	ldr	x18, [x9, #464]
	ldr	x10, [x9, #216]
	msr	tpidr_el0, x10
	ldr	x10, [x9, #224]
	msr	tpidrro_el0, x10
	ldr	x10, [x9, #96]
	mov	sp, x10
	ldr	x0, [x9, #104]
	add	x1, x9, #240
	bl	fki_leave		/* Linux keys, IRQs, preemption back */
	mov	x0, x19
	W(x9)
	ldp	x19, x20, [x9, #0]
	ldp	x21, x22, [x9, #16]
	ldp	x23, x24, [x9, #32]
	ldp	x25, x26, [x9, #48]
	ldp	x27, x28, [x9, #64]
	ldp	x29, x30, [x9, #80]
	ret
	.size	xnu_world_call, . - xnu_world_call

	.globl	xnu_world_fault_abort
	.type	xnu_world_fault_abort, %function
xnu_world_fault_abort:			/* exception return here: XNU keys live, XNU stack */
	W(x9)
	add	x0, x9, #240		/* load Linux keys */
	add	x1, x9, #328		/* save XNU keys */
	bl	fki_pac_swap
	b	xnu_world_abort_mm
	.size	xnu_world_fault_abort, . - xnu_world_fault_abort

	.globl	xnu_world_upcall
	.type	xnu_world_upcall, %function
xnu_world_upcall:			/* x0 = stage, called by XNU with blr */
	hint	#34			/* bti c: module text may be guarded */
	W(x9)
	stp	x19, x20, [x9, #112]
	stp	x21, x22, [x9, #128]
	stp	x23, x24, [x9, #144]
	stp	x25, x26, [x9, #160]
	stp	x27, x28, [x9, #176]
	stp	x29, x30, [x9, #192]
	mov	x10, sp
	str	x10, [x9, #208]
	mov	x19, x0
	mov	x20, x1
	mov	x21, x2
	/*
	 * The current XNU thread's user TLS registers: Linux may schedule during the upcall (idle, file I/O), and its
	 * context switch rewrites TPIDR_EL0 and zeroes TPIDRRO_EL0 (where Darwin user code keeps its TSD pointer).
	 */
	mrs	x10, tpidr_el0
	str	x10, [x9, #448]
	mrs	x10, tpidrro_el0
	str	x10, [x9, #456]
	TO_LINUX_MM
	add	x0, x9, #240		/* load Linux keys */
	add	x1, x9, #328		/* save XNU keys */
	bl	fki_pac_swap
	mov	x0, x19
	mov	x1, x20
	mov	x2, x21
	bl	xnu_hook
	cbnz	x0, xnu_world_abort
	W(x9)
	ldr	x10, [x9, #448]
	msr	tpidr_el0, x10
	ldr	x10, [x9, #456]
	msr	tpidrro_el0, x10
	TO_XNU_MM
	add	x0, x9, #328		/* load XNU keys */
	add	x1, x9, #240		/* save Linux keys */
	bl	fki_pac_swap
	W(x9)
	ldr	x10, [x9, #208]
	mov	sp, x10
	ldp	x19, x20, [x9, #112]
	ldp	x21, x22, [x9, #128]
	ldp	x23, x24, [x9, #144]
	ldp	x25, x26, [x9, #160]
	ldp	x27, x28, [x9, #176]
	ldp	x29, x30, [x9, #192]
	ret
	.size	xnu_world_upcall, . - xnu_world_upcall

/*
 * Deferred XNU kernel fault (see xnu_fault in xnu_main.c): Linux's exception returns here with the faulting context's
 * registers, stack and PSTATE. Save them, let xnu_fault_deferred resolve the fault in XNU's context (it returns where
 * to go and may set x0 for a copyio recovery; frame[31] is the SPSR), then eret there with every register restored.
 */
	.globl	xnu_fault_tramp
	.type	xnu_fault_tramp, %function
xnu_fault_tramp:
	sub	sp, sp, #256
	stp	x0, x1, [sp, #0]
	stp	x2, x3, [sp, #16]
	stp	x4, x5, [sp, #32]
	stp	x6, x7, [sp, #48]
	stp	x8, x9, [sp, #64]
	stp	x10, x11, [sp, #80]
	stp	x12, x13, [sp, #96]
	stp	x14, x15, [sp, #112]
	stp	x16, x17, [sp, #128]
	stp	x18, x19, [sp, #144]
	stp	x20, x21, [sp, #160]
	stp	x22, x23, [sp, #176]
	stp	x24, x25, [sp, #192]
	stp	x26, x27, [sp, #208]
	stp	x28, x29, [sp, #224]
	str	x30, [sp, #240]
	/*
	 * PAN on, as a real exception entry would set it (copyio runs with PAN off; vm_fault may block, and XNU panics on
	 * a context switch with PAN off). The eret below restores the faulting context's PSTATE, PAN included.
	 */
	.inst	0xd500419f		/* msr pan, #1 */
	mov	x0, sp
	bl	xnu_fault_deferred
	msr	elr_el1, x0
	ldr	x1, [sp, #248]
	msr	spsr_el1, x1
	ldp	x0, x1, [sp, #0]
	ldp	x2, x3, [sp, #16]
	ldp	x4, x5, [sp, #32]
	ldp	x6, x7, [sp, #48]
	ldp	x8, x9, [sp, #64]
	ldp	x10, x11, [sp, #80]
	ldp	x12, x13, [sp, #96]
	ldp	x14, x15, [sp, #112]
	ldp	x16, x17, [sp, #128]
	ldp	x18, x19, [sp, #144]
	ldp	x20, x21, [sp, #160]
	ldp	x22, x23, [sp, #176]
	ldp	x24, x25, [sp, #192]
	ldp	x26, x27, [sp, #208]
	ldp	x28, x29, [sp, #224]
	ldr	x30, [sp, #240]
	add	sp, sp, #256
	eret
	.size	xnu_fault_tramp, . - xnu_fault_tramp

/*
 * Pure EL0 host world switch (FKI_CAP_EL0): zero EL1 system instructions.
 * Slot comes from TPIDRRO_EL0 (set to the host CPU id per vCPU).
 * Linux keeps its per-CPU TLS pointer in TPIDR_EL0; XNU keeps its current thread pointer in TPIDR_EL0.
 * Always restore Linux's TPIDR_EL0 before calling into FKI or xnu_hook, and switch PAC keys while Linux's
 * TPIDR_EL0 is live.
 */
#define W_EL0(reg) mrs reg, tpidrro_el0; and reg, reg, #0xff; \
	adrp x10, xnu_worlds; add x10, x10, :lo12:xnu_worlds; ldr reg, [x10, reg, lsl #3]

	.globl	xnu_world_call_el0
	.type	xnu_world_call_el0, %function
xnu_world_call_el0:
	W_EL0(x9)
	str	x18, [x9, #464]
	stp	x19, x20, [x9, #0]
	stp	x21, x22, [x9, #16]
	stp	x23, x24, [x9, #32]
	stp	x25, x26, [x9, #48]
	stp	x27, x28, [x9, #64]
	stp	x29, x30, [x9, #80]
	mov	x10, sp
	str	x10, [x9, #96]
	mov	x19, x0
	mov	x20, x1
	add	x0, x9, #328		/* load XNU keys */
	add	x1, x9, #240		/* save Linux keys */
	bl	fki_enter		/* no preemption, no IRQs */
	W_EL0(x9)
	str	x0, [x9, #104]
	mrs	x10, tpidr_el0
	str	x10, [x9, #216]		/* save Linux TPIDR_EL0 */
	ldr	x10, [x9, #448]
	msr	tpidr_el0, x10		/* load XNU TPIDR_EL0 */
	mov	x0, x20
	blr	x19
	W_EL0(x9)
	mov	x19, #0
	ldr	x10, [x9, #216]
	msr	tpidr_el0, x10		/* restore Linux TPIDR_EL0 before fki_leave */
	b	2f
xnu_world_abort_el0:			/* from xnu_world_upcall_el0: Linux TPIDR_EL0 and keys are already live */
	W_EL0(x9)
	mov	x19, #1
2:	ldr	x18, [x9, #464]
	ldr	x10, [x9, #96]
	mov	sp, x10
	ldr	x0, [x9, #104]
	add	x1, x9, #240
	bl	fki_leave		/* Linux keys, IRQs, preemption back */
	mov	x0, x19
	W_EL0(x9)
	ldp	x19, x20, [x9, #0]
	ldp	x21, x22, [x9, #16]
	ldp	x23, x24, [x9, #32]
	ldp	x25, x26, [x9, #48]
	ldp	x27, x28, [x9, #64]
	ldp	x29, x30, [x9, #80]
	ret
	.size	xnu_world_call_el0, . - xnu_world_call_el0

	.globl	xnu_world_upcall_el0
	.type	xnu_world_upcall_el0, %function
xnu_world_upcall_el0:
	hint	#34			/* bti c */
	W_EL0(x9)
	stp	x19, x20, [x9, #112]
	stp	x21, x22, [x9, #128]
	stp	x23, x24, [x9, #144]
	stp	x25, x26, [x9, #160]
	stp	x27, x28, [x9, #176]
	stp	x29, x30, [x9, #192]
	mov	x10, sp
	str	x10, [x9, #208]
	mov	x19, x0
	mov	x20, x1
	mov	x21, x2
	mrs	x10, tpidr_el0
	str	x10, [x9, #448]		/* save XNU TPIDR_EL0 */
	ldr	x10, [x9, #216]
	msr	tpidr_el0, x10		/* restore Linux TPIDR_EL0 first */
	add	x0, x9, #240		/* load Linux keys */
	add	x1, x9, #328		/* save XNU keys */
	bl	fki_pac_swap
	cbnz	x0, .Lpac_fail_el0
	mov	x0, x19
	mov	x1, x20
	mov	x2, x21
	bl	xnu_hook
	cbnz	x0, xnu_world_abort_el0
	W_EL0(x9)
	add	x0, x9, #328		/* load XNU keys (while Linux TPIDR_EL0 is live) */
	add	x1, x9, #240		/* save Linux keys */
	bl	fki_pac_swap
	cbnz	x0, .Lpac_fail_el0
	W_EL0(x9)
	mrs	x10, tpidr_el0
	str	x10, [x9, #216]
	ldr	x10, [x9, #448]
	msr	tpidr_el0, x10		/* restore XNU TPIDR_EL0 */
	ldr	x10, [x9, #208]
	mov	sp, x10
	ldp	x19, x20, [x9, #112]
	ldp	x21, x22, [x9, #128]
	ldp	x23, x24, [x9, #144]
	ldp	x25, x26, [x9, #160]
	ldp	x27, x28, [x9, #176]
	ldp	x29, x30, [x9, #192]
	ret
/*
 * The host could not switch PAC keys. XNU is not returned to: the run ends as from a hook that says so. After the first
 * swap XNU's keys are still loaded, and fki_leave loads Linux's once more (the host's own keys that cannot be put back
 * are fki's to give up on); after the second, Linux's are. Linux's TPIDR_EL0 is live in both.
 */
.Lpac_fail_el0:
	bl	xnu_world_pac_abort_el0
	b	xnu_world_abort_el0
	.size	xnu_world_upcall_el0, . - xnu_world_upcall_el0

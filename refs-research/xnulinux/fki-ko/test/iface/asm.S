/* SPDX-License-Identifier: GPL-2.0-only */
/* What the foreign side of ifacetest (main.c) does that C cannot say: run on its own stack, use the FP/SIMD registers,
 * fault at a known pc. */
	.text

/* long ifacetest_on_stack(long (*fn)(void *), void *arg, void *top): fn(arg) on the stack that ends at top */
	.globl	ifacetest_on_stack
	.type	ifacetest_on_stack, %function
ifacetest_on_stack:
	hint	#34
	stp	x29, x30, [sp, #-16]!
	mov	x29, sp
	mov	x9, sp
	mov	sp, x2
	stp	x9, xzr, [sp, #-16]!
	mov	x9, x0
	mov	x0, x1
	blr	x9
	ldp	x9, xzr, [sp], #16
	mov	sp, x9
	ldp	x29, x30, [sp], #16
	ret
	.size	ifacetest_on_stack, . - ifacetest_on_stack

/* void ifacetest_simd(unsigned char b): every FP/SIMD register full of b, FPSR and FPCR cleared */
	.globl	ifacetest_simd
	.type	ifacetest_simd, %function
ifacetest_simd:
	hint	#34
	.irp	n,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
	dup	v\n\().16b, w0
	.endr
	msr	fpsr, xzr
	msr	fpcr, xzr
	ret
	.size	ifacetest_simd, . - ifacetest_simd

/* unsigned long ifacetest_peek(unsigned long addr): loads from addr; when that faults, what the fault's handler left in x1 */
	.globl	ifacetest_peek
	.type	ifacetest_peek, %function
ifacetest_peek:
	hint	#34
	mov	x1, #0
	.globl	ifacetest_peek_insn
ifacetest_peek_insn:
	ldr	x1, [x0]
	mov	x0, x1
	ret
	.size	ifacetest_peek, . - ifacetest_peek

/* unsigned long ifacetest_undef(void): an undefined instruction; what its handler left in x1 */
	.globl	ifacetest_undef
	.type	ifacetest_undef, %function
ifacetest_undef:
	hint	#34
	mov	x1, #0
	.globl	ifacetest_undef_insn
ifacetest_undef_insn:
	.inst	0x00000000
	mov	x0, x1
	ret
	.size	ifacetest_undef, . - ifacetest_undef

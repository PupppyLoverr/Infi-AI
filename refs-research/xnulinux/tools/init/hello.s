// SPDX-License-Identifier: GPL-2.0-only
// /sbin/hello for the test root: write a line to /dev/console (fd 0 is not set up: open it) and exit 7.
	.text
	.globl	_start
	.p2align 2
_start:
	mov	x9, sp
	and	sp, x9, #~15
	adr	x0, Lpath
	mov	x1, #1			// O_WRONLY
	mov	x2, #0
	mov	x16, #5			// SYS_open
	svc	#0x80
	b.cs	Lfail
	adr	x1, Lmsg
	mov	x2, #(Lmsg_end - Lmsg)
	mov	x16, #4			// SYS_write
	svc	#0x80
	b.cs	Lfail
	mov	x0, #7
Lexit:
	mov	x16, #1			// SYS_exit
	svc	#0x80
1:	b	1b
Lfail:
	mov	x0, #9
	b	Lexit
Lpath:
	.asciz	"/dev/console"
Lmsg:
	.ascii	"hello from /sbin/hello, exec'd by the child\n"
Lmsg_end:

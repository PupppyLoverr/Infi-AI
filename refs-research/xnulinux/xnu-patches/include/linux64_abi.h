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
 * The interface between a hosted XNU (the LINUX64 machine layer) and its host, xnu.ko.
 *
 * xnu.ko calls arm_init with linux64_hook set to its entry point. XNU calls into the host with `blr linux64_hook`:
 * x0 = the call (enum linux64_call), x1 and x2 = its arguments; x0 on return is 0 to go on, non-zero when the host ends
 * the run. Requests are in XNU memory (the host and XNU share the kernel address space); `result` fields are filled by
 * the host: a count, or a negative Linux errno.
 *
 * linux64_dbg[] (enum linux64_counter) is read by the host's monitor thread on another CPU while XNU runs.
 *
 * This header is shared by both sides and includes nothing: it is copied into XNU's tree (osfmk/arm64) when the
 * patches are applied, and xnu.ko includes it from here.
 */
#ifndef _LINUX64_ABI_H_
#define _LINUX64_ABI_H_

#define LINUX64_ABI_VERSION 5

enum linux64_call {
	LINUX64_CALL_KERNEL_TABLES  = 210,	/* boot: a1 = XNU's kernel L1 table; the host installs its kernel-range entries */
	LINUX64_CALL_KMAP           = 211,	/* EL0 host: a1 = struct linux64_map_req: map [va, va+size) -> pa in high window */
	LINUX64_CALL_KUNMAP         = 212,	/* EL0 host: a1 = struct linux64_map_req: unmap [va, va+size) from high window */
	LINUX64_CALL_KFLUSH         = 213,	/* EL0 host: flush high window TLB */
	LINUX64_CALL_KMAP_TXN       = 214,	/* EL0 host: a1 = struct linux64_map_req: high-window overwrite rollback txn */
	LINUX64_CALL_AS_CREATE      = 220,	/* EL0 host: create user address space */
	LINUX64_CALL_AS_DESTROY     = 221,	/* EL0 host: destroy user address space */
	LINUX64_CALL_AS_MAP         = 222,	/* EL0 host: map pages in user address space */
	LINUX64_CALL_AS_UNMAP       = 223,	/* EL0 host: unmap pages in user address space */
	LINUX64_CALL_AS_FLUSH       = 224,	/* EL0 host: flush user address space TLB */
	/* 225 retired (former LINUX64_CALL_UTCB_RUN before ABI v4 handshake) */
	LINUX64_CALL_UTCB_FPU_SYNC  = 226,	/* EL0 host: flush/invalidate user FPU state */
	LINUX64_CALL_UTCB_RESUME    = 227,	/* EL0 host: resume user context with unchanged GP regs after PF */
	LINUX64_CALL_UTCB_SYNC_REGS = 228,	/* EL0 host: read full GP regs after a lazy PF stop */
	LINUX64_CALL_ICACHE_FLUSH   = 229,	/* EL0 host: a1 = va, a2 = size: cross-CPU icache flush */
	LINUX64_CALL_USER_PAC_OP    = 230,	/* EL0 host: a1 = struct linux64_user_pac_req: user JOP key sign/auth */
	LINUX64_CALL_UTCB_RUN       = 231,	/* EL0 host: run user context */
	LINUX64_CALL_IDLE           = 810,	/* a1 = nanoseconds to XNU's next timer (0: one is due), a2 = this CPU's pending-IPI word
					 * (the host does not sleep while it is set); the host runs Linux meanwhile */
	LINUX64_CALL_PREEMPTED      = 811,	/* a host interrupt is pending: let Linux handle it; a1 = 1 from the kernel, 0 from EL0 */
	LINUX64_CALL_WALLCLOCK      = 812,	/* a1 = u64[2]: {seconds, nanoseconds} UTC */
	LINUX64_CALL_NET_SEND       = 816,	/* a1 = struct linux64_buf_req: one Ethernet frame */
	LINUX64_CALL_NET_RECV       = 817,	/* a1 = struct linux64_buf_req: one Ethernet frame, result 0 when none */
	LINUX64_CALL_INPUT_READ     = 818,	/* a1 = struct linux64_buf_req: whole struct linux64_input_event records */
	LINUX64_CALL_KEYSTORE_RW    = 819,	/* a1 = struct linux64_rw_req on the keystore file */
	LINUX64_CALL_DISK_RW        = 820,	/* a1 = struct linux64_rw_req on the system disk */
	LINUX64_CALL_DISK_SIZE      = 821,	/* a1 = u64 *: the system disk's size in bytes, 0 when there is none */
	LINUX64_CALL_RANDOM         = 822,	/* a1 = buffer, a2 = length: bytes from the host's cryptographic generator */
	LINUX64_CALL_HOSTCALL       = 830,	/* a1 = struct linux64_hostcall_req: a message to a Linux service and its reply */
	LINUX64_CALL_CPU_START      = 840,	/* a1 = a CPU's MPIDR, a2 = its cpu_data_t: the host enters linux64_cpu_entry(a2) there */
	LINUX64_CALL_IPI            = 841,	/* a1 = the target CPU's MPIDR: its linux64_ipi_pending slot is set; make it look */
	LINUX64_CALL_HALT           = 992,	/* a1 = 1 restart, 0 halt: the hosted system shut down (halt_all_cpus); the run ends */
	LINUX64_CALL_CONSOLE_READ   = 993,	/* a1 = struct linux64_buf_req: console input bytes */
	LINUX64_CALL_RAMDISK_RW     = 994,	/* a1 = struct linux64_rw_req on the host-file md disk */
	LINUX64_CALL_RAMDISK_SIZE   = 995,	/* a1 = u64 *: the host-file md disk's size, 0 when there is none */
	LINUX64_CALL_CONSOLE_WRITE  = 996,	/* a1 = bytes, a2 = length: /dev/console output */
	LINUX64_CALL_ASSERT         = 997,	/* a1 = struct linux64_assert_info: a failed assert(); non-zero return ends the run */
	LINUX64_CALL_KPRINTF        = 998,	/* a1 = a NUL-terminated line of kprintf/printf output */
	LINUX64_CALL_PANIC          = 999,	/* a1 = the panic message, a2 = the caller; the run ends */
	LINUX64_CALL_USER_ENTER     = 9100,	/* a1 = user pc: the first entry into user space */
};

/* Page mapping protection bits (match FKI_PROT_*). */
#define LINUX64_PROT_READ  (1 << 0)
#define LINUX64_PROT_WRITE (1 << 1)
#define LINUX64_PROT_EXEC  (1 << 2)

/*
 * Pure EL0 host: where the host maps all of the hosted system's DRAM, linearly, for good (the kernel's physical
 * aperture): the start of the kernel's window (VM_MIN_KERNEL_ADDRESS), below the kernel, so that it takes none of the
 * 48 GiB after the kernel's link address, which are the kernel map's. The image and the boot data (up to
 * topOfKernelData, rounded up to 2 MiB) are mapped a second time at the kernel's base, virtBase.
 */
#define LINUX64_EL0_APERTURE_VA 0xffffffe000000000ULL
#define LINUX64_EL0_APERTURE_END 0xfffffff000000000ULL	/* LOW_GLOBAL_BASE_ADDRESS: the aperture ends at or below it */

/* EL0 host capability bits exported to XNU via linux64_el0_caps. */
#define LINUX64_EL0_CAP_EXEC_ONLY (1U << 0)	/* host supports execute-only (--x) user mappings */
#define LINUX64_EL0_CAP_XO_COMPAT (1U << 1)	/* opt-in: relax --x to r-x when host lacks EXEC_ONLY */

/* High-window overwrite transaction ops (LINUX64_CALL_KMAP_TXN, passed in linux64_map_req.prot). */
enum {
	LINUX64_KMAP_TXN_BEGIN   = 0,
	LINUX64_KMAP_TXN_RESTORE = 1,
	LINUX64_KMAP_TXN_END     = 2,
};

/* Kernel/user page mapping request for EL0 hosts (LINUX64_CALL_KMAP/KUNMAP/KMAP_TXN/AS_*). */
struct linux64_map_req {
	__UINT64_TYPE__ as_handle;
	__UINT64_TYPE__ va;
	__UINT64_TYPE__ pa;
	__UINT64_TYPE__ size;
	__UINT64_TYPE__ prot;
	__INT64_TYPE__ result;
};

/* User-context stop reasons (match FKI_STOP_*). */
enum {
	LINUX64_STOP_SYSCALL   = 1,
	LINUX64_STOP_PAGEFAULT = 2,
	LINUX64_STOP_EXCEPTION = 3,
	LINUX64_STOP_INTERRUPT = 4,
};

/* User-context FPU sync request for EL0 hosts (LINUX64_CALL_UTCB_FPU_SYNC). */
#define LINUX64_FPU_FLUSH      (1U << 0)
#define LINUX64_FPU_INVALIDATE (1U << 1)

struct linux64_fpu_sync_req {
	__UINT64_TYPE__ thread_id;
	__UINT64_TYPE__ neon_ptr;	/* &thread->machine.uNeon->ns_64 (520B) */
	__UINT32_TYPE__ flags;
	__INT32_TYPE__ result;
};

/* User-context execution request for EL0 hosts (LINUX64_CALL_UTCB_RUN). */
struct linux64_utcb_req {
	__UINT64_TYPE__ as_handle;
	__UINT64_TYPE__ thread_id;
	__UINT64_TYPE__ disable_jop;
	__UINT64_TYPE__ rop_pid;
	__UINT64_TYPE__ jop_pid;
	__UINT64_TYPE__ x[31];
	__UINT64_TYPE__ sp, pc, pstate, tpidr, tpidrro;
	__UINT64_TYPE__ neon_ptr;	/* &thread->machine.uNeon->ns_64 (520B) */
	__UINT32_TYPE__ fpu_dirty;
	__INT32_TYPE__ stop_kind;
	__UINT64_TYPE__ fault_addr, fault_ip, esr;
	__INT64_TYPE__ result;
};

/* User JOP-key PAC sign/auth request for EL0 hosts (LINUX64_CALL_USER_PAC_OP). */
enum {
	LINUX64_PAC_KEY_IA = 0,
	LINUX64_PAC_KEY_DA = 2,
};

struct linux64_user_pac_req {
	__UINT64_TYPE__ value;
	__UINT64_TYPE__ discriminator;
	__UINT64_TYPE__ jop_key;
	__UINT64_TYPE__ result;
	__UINT32_TYPE__ key;		/* LINUX64_PAC_KEY_IA or LINUX64_PAC_KEY_DA */
	__UINT32_TYPE__ is_auth;	/* 0: sign, 1: auth */
	__INT32_TYPE__ err;		/* 0 on success, negative Linux errno on PAC swap failure */
	__UINT32_TYPE__ reserved;
};

/* A buffer: bytes to send or room to receive into. */
struct linux64_buf_req {
	void *buf;
	__UINT64_TYPE__ len;
	__INT64_TYPE__ result;
};

/* Reads or writes of a host file at an offset. */
struct linux64_rw_req {
	__UINT64_TYPE__ write;	/* 0: read into buf, 1: write from buf */
	__UINT64_TYPE__ off;
	__UINT64_TYPE__ len;
	void *buf;
	__INT64_TYPE__ result;
};

/* A host call: request and reply in XNU memory. result: the reply's length or -errno. */
struct linux64_hostcall_req {
	const void *req;
	__UINT64_TYPE__ len;
	void *reply;
	__UINT64_TYPE__ max;
	__INT64_TYPE__ result;
};

struct linux64_assert_info {
	const char *file;
	long line;
	const char *expr;
};

/* An input event: Linux evdev's type, code and value. */
struct linux64_input_event {
	unsigned short type;
	unsigned short code;
	int value;
};

/*
 * Per-CPU state shared by XNU and the host lives in arrays indexed by a CPU's slot: affinity 0 and affinity 1 of its
 * MPIDR, four bits each. The host only starts CPUs whose slots differ.
 */
#define LINUX64_CPU_SLOTS 256
#define LINUX64_CPU_SLOT(mpidr) ((unsigned)(((mpidr) & 0xf) | (((mpidr) >> 4) & 0xf0)))

/*
 * The most CPUs the hosted system can have: the size of XNU's per-CPU tables on the virtual board (MAX_CPUS in
 * pexpert's board_config.h, which xnu-patches/0010 sets and osfmk/arm64/machine_routines.c checks against this) and
 * of the host's (xnu.ko gives XNU no more, whatever its cpus parameter asks). A power of two (LINUX64_EL0_CPUS).
 */
#define LINUX64_MAX_CPUS 32

/*
 * linux64_el0_stat[cpu][0][class]: exceptions taken from user space, by class; [cpu][1][class]: counter ticks from
 * taking one until the CPU next enters user space (blocking included). Classes: SVC, data abort, instruction abort,
 * host IRQ, other. By XNU CPU number (cpu_number() & (LINUX64_EL0_CPUS - 1)): one row for each CPU there can be (two
 * CPUs sharing a row would also share its pending start time and class).
 */
#define LINUX64_EL0_CLASSES 5
#define LINUX64_EL0_CPUS LINUX64_MAX_CPUS

enum linux64_counter {
	LINUX64_CTR_IDLE_THREAD  = 0,	/* XNU's idle thread loops */
	LINUX64_CTR_MACHINE_IDLE = 1,	/* machine_idle calls */
	LINUX64_CTR_CPU_IDLE     = 2,	/* cpu_idle calls */
	LINUX64_CTR_HOST_IDLE    = 3,	/* LINUX64_CALL_IDLE calls */
	LINUX64_CTR_SWITCHES     = 4,	/* thread_invoke calls */
	LINUX64_CTR_LAST_CALL    = 6,	/* the last enum linux64_call made */
	LINUX64_CTR_RTCLOCK      = 7,	/* timer interrupts delivered */
	LINUX64_CTR_THREAD       = 9,	/* the thread last switched to */
	LINUX64_CTR_EL0_EXC      = 10,	/* exceptions from user space */
	LINUX64_CTR_EL0_ESR      = 11,	/* the last one's ESR, FAR and PC */
	LINUX64_CTR_EL0_FAR      = 12,
	LINUX64_CTR_EL0_PC       = 13,
	LINUX64_CTR_BAD_ESR      = 14,	/* the ESR of an exception the EL0 vectors could not take */
	LINUX64_CTR_DUMP_REQ     = 15,	/* host to XNU: nonzero asks for a thread dump (linux64_dump_threads) at the next idle */
	LINUX64_NCOUNTERS        = 16,
};

#endif /* _LINUX64_ABI_H_ */

/* SPDX-License-Identifier: GPL-2.0-only WITH Linux-syscall-note */
/*
 * Foreign kernel interface (FKI), version 3.
 *
 * The only interface between Linux and a foreign-kernel module. It is
 * semantic and shaped like a hypervisor interface: plain integer types and
 * opaque handles, no Linux structure crosses it. A foreign module includes
 * this header only.
 */
#ifndef _LINUX_FKI_H
#define _LINUX_FKI_H

#define FKI_VERSION 3

#ifndef __KERNEL__
#include <stdint.h>
typedef uint64_t fki_u64;
#else
#include <linux/types.h>
typedef u64 fki_u64;
#endif

/* Host capability bits returned by fki_caps(). */
#define FKI_CAP_EL0         (1UL << 0)	/* hosted kernel runs at EL0 */
#define FKI_CAP_HIGH_WINDOW (1UL << 1)	/* host manages TTBR1 high window */
#define FKI_CAP_PAC         (1UL << 2)	/* per-context PAC keys supported */
#define FKI_CAP_EXEC_ONLY   (1UL << 3)	/* execute-only (--x) user mappings supported */

/* Page mapping protection bits for fki_high_map() and fki_as_map(). */
#define FKI_PROT_READ       (1 << 0)
#define FKI_PROT_WRITE      (1 << 1)
#define FKI_PROT_EXEC       (1 << 2)

/* Physically contiguous memory, mapped contiguously and executable. */
struct fki_region {
	void *va;
	fki_u64 pa;
	fki_u64 size;
	void *handle;
};

/* Pointer authentication keys (each 128 bits: lo, hi). */
struct fki_pac_keys {
	fki_u64 ia[2], ib[2], da[2], db[2], ga[2];
	fki_u64 sctlr_en;	/* SCTLR_EL1 EnIA|EnIB|EnDA|EnDB bits */
};

/* User-context register state and stop info (FKI_CAP_EL0 hosts). */
struct fki_utcb_regs {
	fki_u64 x[31];
	fki_u64 sp, pc, pstate, tpidr, tpidrro;
};

enum {
	FKI_STOP_SYSCALL   = 1,
	FKI_STOP_PAGEFAULT = 2,
	FKI_STOP_EXCEPTION = 3,
	FKI_STOP_INTERRUPT = 4,
};

struct fki_utcb_stop {
	int kind;
	fki_u64 fault_addr, fault_ip, esr;
};

#ifdef __cplusplus
extern "C" {
#endif
int fki_version(void);
unsigned long fki_caps(void);
int fki_cpu_id(void);
void fki_log(const char *s, unsigned long len);
void fki_log_alert(const char *s, unsigned long len);	/* KERN_ALERT: on the console even when it is quiet */
int fki_region_alloc(unsigned long size, struct fki_region *r);
void fki_region_free(struct fki_region *r);
/* A named blob from the firmware search path. */
int fki_blob_get(const char *name, const void **data, unsigned long *size, void **handle);
void fki_blob_put(void *handle);
/* Enter and leave a foreign context on this CPU: no preemption, no interrupts, foreign PAC keys; the FP/SIMD registers are
 * the foreign kernel's to use (the current task's state saved first). */
unsigned long fki_enter(const struct fki_pac_keys *foreign, struct fki_pac_keys *saved);
void fki_leave(unsigned long cookie, const struct fki_pac_keys *saved);
/* Swap PAC keys only (caller already in a foreign context): save current into *save, load *load. */
int fki_pac_swap(const struct fki_pac_keys *load, struct fki_pac_keys *save);
fki_u64 fki_counter_frequency(void);
fki_u64 fki_counter(void);
void fki_random(void *buf, unsigned long len);
/* sleeps until Linux's generator is seeded (process context): 0, or -errno when a signal comes first */
int fki_random_wait(void);
/*
 * Physically contiguous memory with real physical addresses (for a foreign kernel whose own page tables
 * the hardware walks). va is a kernel linear-map address (RW, not executable). size rounds up to a power of two.
 */
int fki_contig_alloc(unsigned long size, struct fki_region *r);
void fki_contig_free(struct fki_region *r);
/*
 * The top foreign window: the L1 table that Linux's TTBR1 L0 entry for va points to. A foreign kernel may
 * own L1 entries Linux leaves empty in that table; fki_window_set installs one (then a full TLB flush).
 */
int fki_window_l1(unsigned long va, unsigned long long **l1_va, fki_u64 *l1_pa);
void fki_window_set(unsigned long long *slot, fki_u64 desc);
/*
 * Host-managed foreign kernel memory and high-window mappings (FKI_CAP_HIGH_WINDOW):
 * fki_kmem_alloc allocates backing memory (no multi-GB physical contiguity required), maps it at high_va
 * as RWX, associates synthetic physical addresses starting at pa_base, and binds the high window on all CPUs.
 * fki_high_map/unmap/flush mirror additional kernel-VA mappings backed by xpa in [pa_base, pa_base + size).
 * fki_high_map only supports new mappings or in-place same-PFN re-protection; cross-PFN replacement
 * returns -EBUSY and requires the caller to fki_high_unmap while quiescent first.
 * fki_kmem_alias maps the first size bytes of the memory a second time, at va, in the same frames and as RWX and
 * as permanent as at high_va (2 MiB aligned, apart from the first mapping, nothing else mapped there). A later
 * call with the same va and another size grows or shrinks it. Only before the foreign kernel runs.
 */
int fki_kmem_alloc(unsigned long size, unsigned long high_va, fki_u64 pa_base, struct fki_region *r);
int fki_kmem_alias(struct fki_region *r, unsigned long va, unsigned long size);
void fki_kmem_free(struct fki_region *r);
int fki_high_map(unsigned long va, fki_u64 xpa, unsigned long size, int prot);
void fki_high_unmap(unsigned long va, unsigned long size);
void fki_high_flush(void);
int fki_high_txn_begin(unsigned long va, unsigned long size, void **handle_out);
void fki_high_txn_restore(void *txn);
void fki_high_txn_end(void *txn);
int fki_high_read_u64x2(unsigned long va, fki_u64 *out0, fki_u64 *out1);
void fki_icache_flush(unsigned long va, unsigned long size);
/*
 * Host-managed user address spaces and user execution contexts (FKI_CAP_EL0).
 * Passing as == NULL to fki_as_unmap() broadcasts the unmap of [va, va + size)
 * across all active user address spaces (for shared/nested page tables).
 */
void *fki_as_create(void);
void fki_as_destroy(void *as);
int fki_as_map(void *as, unsigned long va, fki_u64 xpa, unsigned long size, int prot);
void fki_as_unmap(void *as, unsigned long va, unsigned long size);
void fki_as_flush(void *as);
void *fki_utcb_create(void *as);
void fki_utcb_destroy(void *utcb);
int fki_utcb_set_as(void *utcb, void *as);
int fki_utcb_set_pac(void *utcb, const struct fki_pac_keys *keys);
int fki_utcb_run(void *utcb, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop);
int fki_utcb_resume(void *utcb, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop);
int fki_utcb_sync_regs(void *utcb, struct fki_utcb_regs *regs);
int fki_utcb_fpu_read(void *utcb, void *buf, unsigned long len);
int fki_utcb_fpu_write(void *utcb, const void *buf, unsigned long len);
/*
 * Called from inside fki_enter() (preemption and IRQs off): let Linux run (IRQs and preemption on)
 * until the counter reaches deadline or max_ns pass, or fki_kick_cpu() is called for this CPU, then return with them off
 * again. Not at all if *wake (when given) is already nonzero: the kicker sets it before kicking.
 */
void fki_idle_until(fki_u64 counter_deadline, fki_u64 max_ns, const volatile unsigned int *wake);
/*
 * Called from inside fki_enter() while the hosted kernel is busy: let Linux take its pending interrupts and
 * reschedule if it must (RCU, other tasks on this CPU), then return with IRQs and preemption off again.
 */
void fki_yield(void);
/* The host's wall clock (UTC): seconds and nanoseconds since 1970. */
void fki_realtime(fki_u64 *sec, fki_u64 *nsec);
/*
 * A virtual Ethernet link to Linux (the netdev "xnu0"): send one frame (header included, no FCS); receive the next
 * frame Linux sent (its length, 0 when none waits); how many frames wait.
 */
long fki_net_send(const void *frame, unsigned long len);
long fki_net_recv(void *buf, unsigned long len);
long fki_net_pending(void);
/* Keep the calling task on one CPU (the hosted kernel sees that CPU's MPIDR). */
int fki_bind_cpu(int cpu);
/* CPUs: how many are online, and a CPU's MPIDR (from ordinary Linux context) */
int fki_num_cpus(void);
int fki_cpu_mpidr(int cpu, fki_u64 *mpidr);
/* Make the foreign kernel on a CPU look at its pending work: wake its idle call, or interrupt it (interrupts may be off) */
void fki_kick_cpu(int cpu);
/*
 * A helper kernel thread bound to one CPU: fn(arg) runs until it returns or fki_thread_stop() is called;
 * fn should loop on !fki_thread_should_stop() and sleep with fki_msleep().
 */
void *fki_thread_start(int (*fn)(void *), void *arg, int cpu, const char *name);
void fki_thread_stop(void *thread);
int fki_thread_should_stop(void);
/* In a helper thread: sleep until *flag is non-zero or the thread is told to stop; fki_thread_wake() wakes it (callable
 * with interrupts off, after setting the flag) */
void fki_thread_wait(const volatile unsigned long *flag);
void fki_thread_wake(void *thread);
void fki_msleep(unsigned int ms);
/*
 * Foreign kernel faults: a kernel-mode data or instruction abort whose address or faulting pc is in [lo, hi) is offered
 * to handler (far, esr, the faulting registers) before Linux's own handling; handler returns 0 when the exception should
 * return (the faulting instruction runs again, or wherever fki_regs_set_pc points it), nonzero to let Linux handle it as
 * usual. One registration at a time.
 */
unsigned long fki_regs_pc(void *regs);
unsigned long fki_regs_lr(void *regs);
unsigned long fki_regs_pstate(void *regs);	/* the interrupted PSTATE (SPSR) */
void fki_regs_set_pc(void *regs, unsigned long pc);
void fki_regs_set_reg(void *regs, unsigned int n, unsigned long v);	/* x0..x30 */
unsigned long fki_regs_reg(void *regs, unsigned int n);			/* x0..x30 */
int fki_fault_register(unsigned long lo, unsigned long hi, int (*handler)(unsigned long far, unsigned long esr, void *regs));
void fki_fault_unregister(void);
/* Read 8 bytes of RAM by physical address (debugging: walking a foreign page table). */
fki_u64 fki_phys_peek(fki_u64 pa);
/*
 * A host file as backing store (a foreign block device's image). fki_file_open is called from ordinary Linux context
 * and returns an opaque handle (NULL on failure) and the file's size. fki_file_rw is called from inside fki_enter():
 * it lets Linux run (IRQs and preemption on) for the I/O, copies through a Linux bounce buffer to or from buf
 * (with IRQs off again), and returns the bytes moved or a negative errno.
 */
void *fki_file_open(const char *path, int writable, fki_u64 *size);
long fki_file_rw(void *file, int write, void *buf, unsigned long len, fki_u64 off);
void fki_file_close(void *file);
/* Write to a host file from ordinary Linux context (a host thread, not inside fki_enter()); bytes written or -errno. */
long fki_file_pwrite(void *file, const void *buf, unsigned long len, fki_u64 off);
/*
 * A host call (from inside fki_enter()): len bytes at req to the Linux service reading /dev/xnu-hostcall, its reply into
 * reply (at most max bytes). Returns the reply's length or a negative errno (-ETIMEDOUT: no service answered).
 */
long fki_hostcall(const void *req, unsigned long len, void *reply, unsigned long max);
#ifdef __cplusplus
}
#endif
#endif

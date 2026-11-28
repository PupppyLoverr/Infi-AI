// SPDX-License-Identifier: GPL-2.0-only
/*
 * What fki exports and xnu.ko relies on, called on a real kernel the way xnu.ko calls it and held against what fki.h
 * says of each call, and against the kernel's own answer where it has one. One group of calls per load (t=); every
 * check is one line in the kernel log, "ok" or "FAIL". run.sh loads fki.ko with the parameters a group needs, makes
 * the file, the firmware blob and the frames a group uses, and reads the verdict. Loading the module is the test; it
 * never stays loaded.
 */
#include <linux/delay.h>
#include <linux/etherdevice.h>
#include <linux/fs.h>
#include <linux/ktime.h>
#include <linux/math64.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/netdevice.h>
#include <linux/skbuff.h>
#include <linux/string.h>
#include <linux/vmalloc.h>
#include <asm/cpufeature.h>
#include <asm/cputype.h>
#include <asm/esr.h>
#include <asm/ptrace.h>
#include <asm/sysreg.h>
#include "fki.h"

static char *t = "", *path = "", *dev = "", *blob = "";
static unsigned long n, devn, pending, send, resv, rounds = 20000;
static long want;
static int cpus[NR_CPUS], cpus_n;	/* fki's cpu_list, when it was loaded with one */
module_param(t, charp, 0);
module_param(path, charp, 0);
module_param(dev, charp, 0);
module_param(blob, charp, 0);
module_param(n, ulong, 0);
module_param(devn, ulong, 0);
module_param(pending, ulong, 0);
module_param(send, ulong, 0);
module_param(resv, ulong, 0);
module_param(rounds, ulong, 0);
module_param(want, long, 0);
module_param_array(cpus, int, &cpus_n, 0);

static int bad;
#define CHECK(cond, fmt, ...) do {						\
	int ok_ = !!(cond);							\
	pr_info("ifacetest: %s: " fmt "\n", ok_ ? "ok" : "FAIL", ##__VA_ARGS__);	\
	bad |= !ok_;								\
} while (0)

long ifacetest_on_stack(long (*fn)(void *), void *arg, void *top);
void ifacetest_simd(unsigned char b);
unsigned long ifacetest_peek(unsigned long addr);
unsigned long ifacetest_undef(void);
extern char ifacetest_peek_insn[], ifacetest_undef_insn[];

/*
 * fn(arg) as host code inside a foreign context, the way xnu.ko runs what XNU asks of it: fki_enter() (no preemption, no
 * interrupts, the foreign keys), then Linux's own keys again for the host code (xnu_world_upcall in xnu-ko/world.S), on
 * a stack that is not the task's, as XNU's are not: where fki lets Linux run it has to find it a stack of Linux's.
 */
#define NO_PAC __attribute__((target("branch-protection=none")))
static const struct fki_pac_keys foreign = { { 0x11, 0x12 }, { 0x21, 0x22 }, { 0x31, 0x32 }, { 0x41, 0x42 }, { 0x51, 0x52 }, ~0ULL };
static void *stack_mem, *stack_top;

static NO_PAC long in_foreign(long (*fn)(void *), void *arg, struct fki_pac_keys *seen)
{
	struct fki_pac_keys saved, mine, tmp;
	unsigned long cookie;
	long r;

	cookie = fki_enter(&foreign, &saved);
	fki_pac_swap(&saved, &mine);
	r = ifacetest_on_stack(fn, arg, stack_top);
	fki_pac_swap(&mine, &tmp);
	fki_leave(cookie, &saved);
	if (seen)
		*seen = mine;
	return r;
}

static int map(int c)	/* the Linux CPU of the foreign kernel's CPU c */
{
	if (!cpus_n)
		return c;
	return c >= 0 && c < cpus_n ? cpus[c] : -1;
}

static int is_cpu(int c)
{
	return c >= 0 && c < nr_cpu_ids && cpu_online(c);
}

/* ---- entering and leaving ---- */

struct inside { int irqs_off, preempt, cpu, fki_cpu, own_stack; };

static long inside_fn(void *p)
{
	struct inside *s = p;

	s->own_stack = (void *)&s >= stack_top - THREAD_SIZE && (void *)&s < stack_top;
	s->irqs_off = irqs_disabled();
	s->preempt = preempt_count();
	s->cpu = smp_processor_id();
	s->fki_cpu = fki_cpu_id();
	return 0;
}

static void t_enter(void)
{
	unsigned long caps = fki_caps();
	int pac = system_supports_address_auth(), pre = preempt_count();
	struct fki_pac_keys seen = {};
	struct inside s = {};
	u64 k0[2] = {}, k1[2] = {};

	CHECK(fki_version() == FKI_VERSION, "fki_version() = %d, fki.h is version %d", fki_version(), FKI_VERSION);
	CHECK(!(caps & (FKI_CAP_EL0 | FKI_CAP_HIGH_WINDOW)) && !!(caps & FKI_CAP_PAC) == pac,
	      "fki_caps() = %#lx: a host at EL1, per-context pointer authentication keys %s (this CPU has %s)",
	      caps, caps & FKI_CAP_PAC ? "offered" : "not offered", pac ? "them" : "none");
	if (pac) {
		k0[0] = read_sysreg_s(SYS_APIAKEYLO_EL1);
		k0[1] = read_sysreg_s(SYS_APIAKEYHI_EL1);
	}
	in_foreign(inside_fn, &s, &seen);
	if (pac) {
		k1[0] = read_sysreg_s(SYS_APIAKEYLO_EL1);
		k1[1] = read_sysreg_s(SYS_APIAKEYHI_EL1);
	}
	CHECK(s.irqs_off && s.preempt != pre && s.own_stack, "inside fki_enter(): interrupts %s, preempt count %#x (%#x outside), on %s stack",
	      s.irqs_off ? "off" : "ON", s.preempt, pre, s.own_stack ? "the test's own" : "the TASK's");
	CHECK(s.fki_cpu == s.cpu, "fki_cpu_id() = %d on Linux's CPU %d", s.fki_cpu, s.cpu);
	if (pac) {
		CHECK(!memcmp(&seen, &foreign, offsetof(struct fki_pac_keys, sctlr_en)) && seen.sctlr_en,
		      "the keys loaded inside are the foreign ones (APIA %#llx:%#llx, given %#llx:%#llx)",
		      seen.ia[1], seen.ia[0], foreign.ia[1], foreign.ia[0]);
		CHECK(k0[0] == k1[0] && k0[1] == k1[1], "after fki_leave(): Linux's own key again");
	}
	CHECK(!irqs_disabled() && preempt_count() == pre, "after fki_leave(): interrupts %s, preempt count %#x",
	      irqs_disabled() ? "OFF" : "on", preempt_count());
}

/* ---- the counter and the wall clock ---- */

static void t_clock(void)
{
	u64 f = fki_counter_frequency(), c0, c1, c, prev, sec, nsec;
	struct timespec64 ts;
	int i, back = 0;
	ktime_t k0, k1;
	s64 kns, cns;

	CHECK(f, "fki_counter_frequency() = %llu Hz", f);
	if (!f)
		return;
	preempt_disable(); c0 = fki_counter(); k0 = ktime_get(); preempt_enable();
	msleep(250);
	preempt_disable(); c1 = fki_counter(); k1 = ktime_get(); preempt_enable();
	kns = ktime_to_ns(ktime_sub(k1, k0));
	cns = div64_u64((c1 - c0) * NSEC_PER_SEC, f);
	CHECK(abs(cns - kns) * 100 < kns, "%lld ns by Linux's clock are %lld ns by fki_counter() at that frequency", kns, cns);
	for (i = 0, prev = fki_counter(); i < 100000; i++, prev = c) {
		c = fki_counter();
		if (c < prev)
			back++;
	}
	CHECK(!back, "100000 readings of fki_counter(): %d went backwards", back);
	fki_realtime(&sec, &nsec);
	ktime_get_real_ts64(&ts);
	CHECK(abs((s64)sec - ts.tv_sec) <= 1 && nsec < NSEC_PER_SEC, "fki_realtime() = %llu.%09llu, Linux's wall clock %lld.%09ld",
	      sec, nsec, (s64)ts.tv_sec, ts.tv_nsec);
}

/* ---- fki_idle_until() and fki_kick_cpu() ---- */

struct idle { u64 ns, max_ns; const volatile unsigned int *wake; int past; s64 us; int atomic; };

static long idle_fn(void *p)
{
	struct idle *i = p;
	u64 f = fki_counter_frequency();
	ktime_t t0 = ktime_get();

	fki_idle_until(i->past ? fki_counter() - 1 : fki_counter() + div64_u64(i->ns * f, NSEC_PER_SEC), i->max_ns, i->wake);
	i->us = ktime_us_delta(ktime_get(), t0);
	i->atomic = irqs_disabled() && preempt_count();
	return 0;
}

static struct kick { unsigned long go; unsigned int wake, ms; int cpu, set_wake; } kick;

static int kick_fn(void *arg)
{
	fki_thread_wait(&kick.go);
	if (!READ_ONCE(kick.go))
		return 0;
	fki_msleep(kick.ms);
	if (kick.set_wake)
		WRITE_ONCE(kick.wake, 1);
	fki_kick_cpu(kick.cpu);
	return 0;
}

/* the idle call of the foreign kernel's CPU 0 (this task) with a kick from its CPU `from` after ms */
static s64 kicked(int from, unsigned int ms, int set_wake, const volatile unsigned int *wake)
{
	struct idle i = { .ns = 5 * NSEC_PER_SEC, .max_ns = 5 * NSEC_PER_SEC, .wake = wake };
	void *th;

	kick = (struct kick){ .ms = ms, .cpu = 0, .set_wake = set_wake };
	th = fki_thread_start(kick_fn, NULL, from, "ifacetest-kick");
	if (!th)
		return -1;
	WRITE_ONCE(kick.go, 1);
	fki_thread_wake(th);
	in_foreign(idle_fn, &i, NULL);
	fki_thread_stop(th);
	return i.us;
}

static struct hammer { unsigned long round; unsigned int wake; } hammer;

static int hammer_fn(void *arg)
{
	unsigned long seen = 0, r, i;

	while (!fki_thread_should_stop()) {
		r = READ_ONCE(hammer.round);
		if (r == seen) {
			cond_resched();
			continue;
		}
		seen = r;
		for (i = (r * 7919) % 300; i; i--)	/* the kick comes before, at and after the other CPU's going to sleep */
			cpu_relax();
		WRITE_ONCE(hammer.wake, 1);
		fki_kick_cpu(0);
	}
	return 0;
}

struct hammered { unsigned long late; s64 worst_us; };

static long hammer_idle_fn(void *p)
{
	struct hammered *h = p;
	u64 f = fki_counter_frequency();
	unsigned long r;

	for (r = 1; r <= rounds; r++) {
		ktime_t t0;
		s64 us;

		WRITE_ONCE(hammer.wake, 0);
		t0 = ktime_get();
		WRITE_ONCE(hammer.round, r);
		fki_idle_until(fki_counter() + f / 2, 500 * NSEC_PER_MSEC, &hammer.wake);
		us = ktime_us_delta(ktime_get(), t0);
		if (us > h->worst_us)
			h->worst_us = us;
		if (us > 250000)
			h->late++;
		while (!READ_ONCE(hammer.wake))		/* (a kick that woke the call before its word was set) */
			fki_yield();
	}
	return 0;
}

static void t_idle(void)
{
	struct idle i;
	struct hammered h = {};
	unsigned int one = 1;
	int ncpu = fki_num_cpus(), from = ncpu > 1 ? 1 : 0, err;
	void *th;
	s64 us;

	err = fki_bind_cpu(0);
	CHECK(!err && raw_smp_processor_id() == map(0), "fki_bind_cpu(0) = %d: this task is on Linux's CPU %d (the foreign kernel's CPU 0 is %d)",
	      err, raw_smp_processor_id(), map(0));
	if (err)
		return;
	i = (struct idle){ .ns = 50 * NSEC_PER_MSEC, .max_ns = NSEC_PER_SEC };
	in_foreign(idle_fn, &i, NULL);
	CHECK(i.us >= 49000 && i.us < 150000 && i.atomic, "fki_idle_until(a deadline in 50 ms, at most 1 s): back after %lld us, interrupts and preemption %s",
	      i.us, i.atomic ? "off again" : "NOT off again");
	i = (struct idle){ .ns = 5 * NSEC_PER_SEC, .max_ns = 30 * NSEC_PER_MSEC };
	in_foreign(idle_fn, &i, NULL);
	CHECK(i.us >= 29000 && i.us < 130000, "fki_idle_until(a deadline in 5 s, at most 30 ms): back after %lld us", i.us);
	i = (struct idle){ .past = 1, .max_ns = NSEC_PER_SEC };
	in_foreign(idle_fn, &i, NULL);
	CHECK(i.us < 20000 && i.atomic, "fki_idle_until(a deadline that has passed): back after %lld us", i.us);
	i = (struct idle){ .ns = 5 * NSEC_PER_SEC, .max_ns = 5 * NSEC_PER_SEC, .wake = &one };
	in_foreign(idle_fn, &i, NULL);
	CHECK(i.us < 20000, "fki_idle_until(a deadline in 5 s, a wake word already set): back after %lld us", i.us);

	us = kicked(from, 40, 1, &kick.wake);
	CHECK(us >= 0 && us < 1000000, "fki_idle_until(5 s) and, 40 ms later, fki_kick_cpu(0) with the wake word set, from the foreign CPU %d: back after %lld us", from, us);
	us = kicked(from, 40, 0, NULL);
	CHECK(us >= 0 && us < 1000000, "the same without a wake word: back after %lld us", us);

	th = fki_thread_start(hammer_fn, NULL, from, "ifacetest-kick");
	if (th) {
		in_foreign(hammer_idle_fn, &h, NULL);
		fki_thread_stop(th);
	}
	CHECK(th && !h.late, "%lu times fki_idle_until(500 ms) with the kick (word set first) arriving around the moment it goes to sleep: %lu slept on past the kick, the longest call %lld us",
	      rounds, h.late, h.worst_us);
}

/* ---- CPUs and helper threads ---- */

struct where { int cpu; u64 mpidr; unsigned long done; };

static int where_fn(void *arg)
{
	struct where *w = arg;

	w->cpu = smp_processor_id();
	w->mpidr = read_cpuid_mpidr();
	WRITE_ONCE(w->done, 1);
	return 0;
}

static int until(const unsigned long *flag, int ms)
{
	while (!READ_ONCE(*flag) && ms-- > 0)
		msleep(1);
	return READ_ONCE(*flag) != 0;
}

static struct waiter { unsigned long flag, woke, stopped; } waiter;

static int waiter_fn(void *arg)
{
	fki_thread_wait(&waiter.flag);
	WRITE_ONCE(waiter.woke, READ_ONCE(waiter.flag) ? 1 : 2);	/* 2: told to stop while it waited */
	while (!fki_thread_should_stop())
		fki_msleep(1);
	WRITE_ONCE(waiter.stopped, 1);
	return 0;
}

static void t_cpus(void)
{
	int ncpu = fki_num_cpus(), c, err;
	struct where w;
	fki_u64 m;
	void *th;

	CHECK(ncpu == (cpus_n ? cpus_n : num_online_cpus()), "fki_num_cpus() = %d (%s)", ncpu, cpus_n ? "the length of cpu_list" : "the CPUs online");
	for (c = -1; c <= ncpu; c++) {
		w = (struct where){ .cpu = -1 };
		m = ~0ULL;
		th = fki_thread_start(where_fn, &w, c, "ifacetest");
		err = fki_cpu_mpidr(c, &m);
		if (is_cpu(map(c))) {
			until(&w.done, 1000);
			CHECK(th && w.cpu == map(c) && !err && m == w.mpidr,
			      "the foreign kernel's CPU %d: fki_thread_start() ran its thread on Linux's CPU %d (to be %d); fki_cpu_mpidr() = %d, %#llx (that CPU reads %#llx)",
			      c, w.cpu, map(c), err, m, w.mpidr);
		} else {
			CHECK(!th && err == -EINVAL, "the foreign kernel's CPU %d, which is no CPU (Linux's %d): fki_thread_start() gave %s, fki_cpu_mpidr() = %d",
			      c, map(c), th ? "a thread" : "none", err);
			fki_kick_cpu(c);	/* nothing to kick, and nothing must happen */
		}
		fki_thread_stop(th);	/* (it has returned by itself: allowed) */
	}
	err = fki_bind_cpu(ncpu);
	CHECK(err == -EINVAL, "fki_bind_cpu(%d) = %d", ncpu, err);
	for (c = 0; c < ncpu && !is_cpu(map(c)); c++)
		;
	if (c == ncpu)
		return;
	err = fki_bind_cpu(c);
	CHECK(!err && raw_smp_processor_id() == map(c) && current->nr_cpus_allowed == 1, "fki_bind_cpu(%d) = %d: this task is on Linux's CPU %d and stays there", c, err, raw_smp_processor_id());

	waiter = (struct waiter){ };
	th = fki_thread_start(waiter_fn, NULL, c, "ifacetest");
	msleep(30);
	CHECK(th && !READ_ONCE(waiter.woke), "a thread in fki_thread_wait() with its flag clear: %s", READ_ONCE(waiter.woke) ? "did NOT wait" : "waits");
	WRITE_ONCE(waiter.flag, 1);
	fki_thread_wake(th);
	CHECK(until(&waiter.woke, 1000) && waiter.woke == 1, "the flag set and fki_thread_wake(): %s", waiter.woke ? "it goes on" : "it still WAITS");
	fki_thread_stop(th);
	CHECK(READ_ONCE(waiter.stopped), "fki_thread_stop(): the thread saw fki_thread_should_stop() and %s", waiter.stopped ? "returned" : "did NOT return");
	waiter = (struct waiter){ };
	th = fki_thread_start(waiter_fn, NULL, c, "ifacetest");
	msleep(30);
	fki_thread_stop(th);
	CHECK(waiter.woke == 2 && waiter.stopped, "fki_thread_stop() of a thread in fki_thread_wait(): %s", waiter.stopped ? "it returned" : "it did NOT return");
}

/* ---- the Ethernet link ---- */

#define TEST_PROTO 0x88b5	/* an ethertype for local experiments: the frames of this test (netpeer.c has the same) */
static const unsigned short lens[4] = { 60, 61, 590, 1514 };
static unsigned char *fbuf;

static void frame_fill(unsigned char *f, unsigned int proto, unsigned int seq, unsigned int len)
{
	unsigned int i;

	memset(f, 0xff, 6);
	memcpy(f + 6, "\x02xnut", 6);
	f[12] = proto >> 8; f[13] = proto;
	for (i = 14; i < len; i++)
		f[i] = i < 18 ? seq >> (8 * (17 - i)) : seq * 13 + i;
}

static int frame_ok(const unsigned char *f, long len, unsigned int seq)
{
	unsigned int i;

	if (len != lens[seq % 4])
		return 0;
	for (i = 14; i < len; i++)
		if (f[i] != (unsigned char)(i < 18 ? seq >> (8 * (17 - i)) : seq * 13 + i))
			return 0;
	return 1;
}

struct link { long pending0, good, wrong, other, errs, empty, pending1, sent, short_r, long_r, min_r, max_r; };

static long net_fn(void *p)
{
	struct link *s = p;
	unsigned int i;
	long r;

	s->pending0 = fki_net_pending();
	for (i = 0; i < 8192; i++) {
		r = fki_net_recv(fbuf, 2048);
		if (!r)
			break;
		if (r < 0)
			s->errs++;
		else if (r < 18 || fbuf[12] != TEST_PROTO >> 8 || fbuf[13] != (TEST_PROTO & 0xff))
			s->other++;
		else if (frame_ok(fbuf, r, s->good))	/* in the order they were sent */
			s->good++;
		else
			s->wrong++;
	}
	s->empty = fki_net_recv(fbuf, 2048);
	s->pending1 = fki_net_pending();
	for (i = 0; i < send; i++) {
		frame_fill(fbuf, TEST_PROTO, i, lens[i % 4]);
		if (fki_net_send(fbuf, lens[i % 4]) == lens[i % 4])
			s->sent++;
	}
	frame_fill(fbuf, TEST_PROTO + 1, 0, 1519);
	s->short_r = fki_net_send(fbuf, 13);
	s->long_r = fki_net_send(fbuf, 1519);
	s->min_r = fki_net_send(fbuf, 14);
	s->max_r = fki_net_send(fbuf, 1518);
	return 0;
}

static void t_net(void)
{
	struct link s = {};

	in_foreign(net_fn, &s, NULL);
	CHECK(s.good == pending && !s.wrong && !s.errs, "fki_net_recv(): %ld of the %lu frames Linux sent on xnu0, in order and whole; %ld damaged, %ld errors (and %ld frames of other kinds)",
	      s.good, pending, s.wrong, s.errs, s.other);
	CHECK(s.pending0 == s.good + s.wrong + s.errs + s.other, "fki_net_pending() before them = %ld", s.pending0);
	CHECK(!s.empty && !s.pending1, "with none left: fki_net_recv() = %ld, fki_net_pending() = %ld", s.empty, s.pending1);
	CHECK(s.sent == send, "fki_net_send() took %ld of %lu frames (60 to 1514 bytes)", s.sent, send);
	CHECK(s.short_r < 0 && s.long_r < 0 && s.min_r == 14 && s.max_r == 1518,
	      "fki_net_send() of 13, 14, 1518 and 1519 bytes = %ld, %ld, %ld, %ld", s.short_r, s.min_r, s.max_r, s.long_r);
}

static struct net_device *xnu0;
static int xmit_err;

static int xmit_fn(void *arg)
{
	struct sk_buff *skb;

	fki_msleep(100);
	skb = alloc_skb(LL_RESERVED_SPACE(xnu0) + 60, GFP_KERNEL);
	if (!skb) {
		xmit_err = -ENOMEM;
		return 0;
	}
	skb_reserve(skb, LL_RESERVED_SPACE(xnu0));
	skb_reset_mac_header(skb);
	frame_fill(skb_put(skb, 60), TEST_PROTO, 0, 60);
	skb_set_network_header(skb, ETH_HLEN);
	skb->dev = xnu0;
	skb->protocol = htons(TEST_PROTO);
	xmit_err = dev_queue_xmit(skb);
	return 0;
}

/* Linux sends a frame while the foreign kernel's first CPU idles: the idle call ends for it (fki_net_xmit) */
static void t_netwake(void)
{
	struct idle i = { .ns = 2 * NSEC_PER_SEC, .max_ns = 2 * NSEC_PER_SEC };
	struct link s = {};
	int ncpu = fki_num_cpus(), err;
	void *th;

	xnu0 = dev_get_by_name(&init_net, "xnu0");
	if (!xnu0 || !netif_running(xnu0)) {
		CHECK(0, "xnu0 is %s", xnu0 ? "not up" : "not there");
		goto out;
	}
	err = fki_bind_cpu(0);
	if (err) {
		CHECK(0, "fki_bind_cpu(0) = %d", err);
		goto out;
	}
	th = fki_thread_start(xmit_fn, NULL, ncpu - 1, "ifacetest-xmit");
	in_foreign(idle_fn, &i, NULL);
	fki_thread_stop(th);
	send = 0;
	in_foreign(net_fn, &s, NULL);
	CHECK(th && !xmit_err && i.us < 1000000 && s.good == 1,
	      "the foreign kernel's CPU 0 (Linux's %d) in fki_idle_until(2 s), Linux sends a frame on xnu0 100 ms later: the call is back after %lld us; the frame %s",
	      raw_smp_processor_id(), i.us, s.good == 1 ? "waits" : "is NOT there");
out:
	if (xnu0)
		dev_put(xnu0);
}

/* ---- files ---- */

static long kread(const char *name, void *buf, unsigned long len, loff_t off, loff_t *size)
{
	struct file *f = filp_open(name, O_RDONLY | O_LARGEFILE, 0);
	long r;

	if (IS_ERR(f))
		return PTR_ERR(f);
	r = kernel_read(f, buf, len, &off);
	if (size)
		*size = i_size_read(f->f_mapping->host);
	filp_close(f, NULL);
	return r;
}

struct io { void *file; int write; void *buf; unsigned long len; fki_u64 off; long r; int atomic; };

static long io_fn(void *p)
{
	struct io *o;

	for (o = p; o->buf; o++) {
		o->r = fki_file_rw(o->file, o->write, o->buf, o->len, o->off);
		o->atomic = irqs_disabled() && preempt_count();
	}
	return 0;
}

static void t_file(void)
{
	unsigned char *a = vmalloc(n + 4096), *b = vmalloc(n + 8192);
	unsigned long big = 300000, i;
	fki_u64 size = ~0ULL;
	loff_t ksize = 0;
	void *f, *fw;
	long kr;

	if (!a || !b || n < big + 4096)
		goto out;
	f = fki_file_open(path, 0, &size);
	CHECK(f && size == n, "fki_file_open(a file of %lu bytes, to read): %s, size %llu", n, f ? "a handle" : "NO handle", size);
	if (!f)
		goto out;
	{
		struct io o[] = {
			{ f, 0, a, n, 0 }, { f, 0, b, 4096, n - 100 }, { f, 0, b, 4096, n }, { f, 0, b, 4096, n + (1 << 20) },
			{ f, 1, a, 4096, 0 }, { NULL, 0, b, 4096, 0 }, { }
		};

		in_foreign(io_fn, o, NULL);
		kr = kread(path, b, n, 0, NULL);
		CHECK(o[0].r == n && kr == n && !memcmp(a, b, n) && o[0].atomic,
		      "fki_file_rw(read all of it) = %ld: %s the kernel's own read of the file; interrupts and preemption %s after it",
		      o[0].r, kr == n && !memcmp(a, b, n) ? "the same bytes as" : "NOT the bytes of", o[0].atomic ? "off again" : "NOT off again");
		CHECK(o[1].r == 100, "fki_file_rw(read 4096 bytes, 100 before the end) = %ld", o[1].r);
		CHECK(o[2].r == 0 && o[3].r == 0, "fki_file_rw(read at the end, and 1 MiB past it) = %ld, %ld", o[2].r, o[3].r);
		CHECK(o[4].r < 0, "fki_file_rw(write) on a file opened to read = %ld", o[4].r);
		CHECK(o[5].r == -EBADF, "fki_file_rw() without a file = %ld", o[5].r);
	}
	fki_file_close(f);

	fw = fki_file_open(path, 1, NULL);
	CHECK(fw, "fki_file_open(the file, to write): %s", fw ? "a handle" : "NO handle");
	if (!fw)
		goto out;
	for (i = 0; i < big; i++)
		a[i] = i * 37 + (i >> 9);
	{
		struct io o[] = { { fw, 1, a, big, 5 }, { fw, 1, a, 100, n + 4096 }, { fw, 0, b, big, 5 }, { } };

		in_foreign(io_fn, o, NULL);
		CHECK(o[0].r == big && o[2].r == big && !memcmp(a, b, big), "fki_file_rw(write %lu bytes at 5) = %ld, read back = %ld: %s", big, o[0].r, o[2].r,
		      !memcmp(a, b, big) ? "the same bytes" : "NOT the same bytes");
		kr = kread(path, b, n + 8192, 0, &ksize);
		CHECK(o[1].r == 100 && ksize == n + 4196 && kr == ksize && !memcmp(b + 5, a, big) && !memcmp(b + n + 4096, a, 100) && !memchr_inv(b + n, 0, 4096),
		      "fki_file_rw(write 100 bytes 4096 past the end) = %ld: the kernel's own read finds %lld bytes (to be %lu), both writes in place, zeros between", o[1].r, ksize, n + 4196);
	}
	kr = fki_file_pwrite(fw, "pwrite", 6, 1);
	kread(path, b, 16, 0, NULL);
	CHECK(kr == 6 && !memcmp(b + 1, "pwrite", 6), "fki_file_pwrite(6 bytes at 1) = %ld: %s", kr, !memcmp(b + 1, "pwrite", 6) ? "in the file" : "NOT in the file");
	kr = fki_file_pwrite(NULL, "x", 1, 0);
	CHECK(kr == -EBADF, "fki_file_pwrite() without a file = %ld", kr);
	fki_file_close(fw);
	f = fki_file_open(path, 0, NULL);
	kr = fki_file_pwrite(f, "x", 1, 0);
	CHECK(f && kr == -EBADF, "fki_file_pwrite() on a file opened to read = %ld", kr);
	fki_file_close(f);
	fki_file_close(NULL);
	f = fki_file_open("/nonexistent/ifacetest", 0, &size);
	CHECK(!f, "fki_file_open(a path that is not there): %s", f ? "a handle" : "no handle");
	if (*dev) {
		size = ~0ULL;
		f = fki_file_open(dev, 0, &size);
		CHECK(f && size == devn, "fki_file_open(%s, a block device of %lu bytes): %s, size %llu", dev, devn, f ? "a handle" : "NO handle", size);
		if (f) {
			struct io o[] = { { f, 0, a, 4096, devn - 4096 }, { f, 0, b, 4096, devn }, { } };

			in_foreign(io_fn, o, NULL);
			kr = kread(dev, b, 4096, devn - 4096, NULL);
			CHECK(o[0].r == 4096 && kr == 4096 && !memcmp(a, b, 4096) && o[1].r == 0, "fki_file_rw(read its last 4096 bytes, and at its end) = %ld, %ld", o[0].r, o[1].r);
			fki_file_close(f);
		}
	}
out:
	vfree(a); vfree(b);
}

/* ---- firmware blobs ---- */

static void t_blob(void)
{
	unsigned char *b = vmalloc(n + 4096);
	char name[128], none[128];
	const void *data = NULL;
	unsigned long size = 0;
	void *handle = NULL;
	long kr;
	int err;

	if (!b)
		return;
	snprintf(name, sizeof(name), "/lib/firmware/%s", blob);
	snprintf(none, sizeof(none), "%s.none", blob);
	kr = kread(name, b, n + 4096, 0, NULL);
	err = fki_blob_get(blob, &data, &size, &handle);
	CHECK(!err && size == n && kr == n && data && !memcmp(data, b, n), "fki_blob_get(\"%s\") = %d: %lu bytes, %s the kernel's own read of %s (%ld bytes)",
	      blob, err, size, !err && kr == n && size == n && !memcmp(data, b, n) ? "the same as" : "NOT", name, kr);
	if (!err)
		fki_blob_put(handle);
	err = fki_blob_get(none, &data, &size, &handle);
	CHECK(err < 0, "fki_blob_get(a name that is not there) = %d", err);
	vfree(b);
}

/* ---- memory ---- */

static unsigned long free_mb(void)
{
	struct sysinfo si;

	si_meminfo(&si);
	return si.freeram >> (20 - PAGE_SHIFT);
}

static void t_mem(void)
{
	static const u32 code[] = { 0xd503245f /* bti c */, 0x52800540 /* mov w0, #42 */, 0xd65f03c0 /* ret */ };
	struct fki_region r = {}, c = {}, d = {};
	unsigned long before, after;
	fki_u64 first;
	int err, i, v = 0, zero;

	err = fki_region_alloc(3 * PAGE_SIZE + 1, &r);
	zero = !err && !memchr_inv(r.va, 0, r.size);
	CHECK(!err && r.va && r.size == 4 * PAGE_SIZE && zero && r.pa == page_to_phys(vmalloc_to_page(r.va)),
	      "fki_region_alloc(3 pages and a byte) = %d: %llu bytes, %s, pa %#llx (its first page is at %#llx)", err, r.size, zero ? "zeroed" : "NOT zeroed",
	      r.pa, err ? 0 : (u64)page_to_phys(vmalloc_to_page(r.va)));
	if (!err) {
		memcpy((char *)r.va + 3 * PAGE_SIZE, code, sizeof(code));
		fki_icache_flush((unsigned long)r.va, r.size);
		v = ((int (*)(void))((char *)r.va + 3 * PAGE_SIZE))();
		CHECK(v == 42, "code written into its last page and called: returned %d (to be 42)", v);
		fki_region_free(&r);
	}
	before = free_mb();
	for (i = 0; i < 200 && !err; i++) {
		err = fki_region_alloc(1 << 20, &r);
		if (!err)
			fki_region_free(&r);
	}
	after = free_mb();
	CHECK(!err && after + 64 > before, "200 times fki_region_alloc(1 MiB) and fki_region_free(): free memory %lu MiB before, %lu MiB after", before, after);

	err = fki_contig_alloc(3 << 20, &c);
	zero = !err && !memchr_inv(c.va, 0, c.size);
	CHECK(!err && c.size == 4 << 20 && virt_to_phys(c.va) == c.pa && !(c.pa & ((2 << 20) - 1)) && zero,
	      "fki_contig_alloc(3 MiB) = %d: %llu bytes (a power of two) at pa %#llx, %s, va %s its address in Linux's map of memory", err, c.size, c.pa,
	      zero ? "zeroed" : "NOT zeroed", !err && virt_to_phys(c.va) == c.pa ? "is" : "is NOT");
	if (!err) {
		memset(c.va, 0xa5, c.size);
		fki_contig_free(&c);
	}
	before = free_mb();
	for (i = 0; i < 50 && !err; i++) {
		err = fki_contig_alloc(4 << 20, &c);
		if (!err) {
			memset(c.va, 0xa5, c.size);
			fki_contig_free(&c);
		}
	}
	after = free_mb();
	CHECK(!err && after + 64 > before, "50 times fki_contig_alloc(4 MiB), written, and fki_contig_free(): free memory %lu MiB before, %lu MiB after (the last result %d)", before, after, err);
	if (!resv)
		return;
	/* fki.ko was loaded with reserve_mb=resv: the reservation is what the first call of its size gets, and gets again */
	err = fki_contig_alloc(resv << 20, &c);
	first = c.pa;
	i = fki_contig_alloc(resv << 20, &d);
	CHECK(!err && c.size == resv << 20 && !memchr_inv(c.va, 0, c.size) && (i || d.pa != c.pa),
	      "fki_contig_alloc(%lu MiB, what reserve_mb set aside) = %d at pa %#llx; a second one while it is held = %d%s", resv, err, c.pa, i, !i && d.pa == c.pa ? " at the SAME address" : "");
	if (!i)
		fki_contig_free(&d);
	if (err)
		return;
	memset(c.va, 0xa5, c.size);
	fki_contig_free(&c);
	err = fki_contig_alloc(resv << 20, &c);
	CHECK(!err && c.pa == first && !memchr_inv(c.va, 0, c.size), "freed and asked for again = %d: at pa %#llx (the reservation is at %#llx), %s", err, c.pa, first,
	      !err && !memchr_inv(c.va, 0, c.size) ? "zeroed" : "NOT zeroed");
	if (!err)
		fki_contig_free(&c);
}

/* fki.ko was loaded with a carve-out (carve_base=, carve_mb=) that run.sh chose to be refused: want is the error */
static void t_carve(void)
{
	struct fki_region c = {};
	int err = fki_contig_alloc(2 << 20, &c);

	CHECK(err == want, "fki_contig_alloc(2 MiB) with that carve-out = %d (to be %ld)", err, want);
	if (!err)
		fki_contig_free(&c);
}

/* ---- kernel faults of the foreign kernel ---- */

static struct fault { unsigned long calls, far, esr, pc, pstate, x0; } flt;

static int fault_handler(unsigned long far, unsigned long esr, void *regs)
{
	flt.calls++;
	flt.far = far;
	flt.esr = esr;
	flt.pc = fki_regs_pc(regs);
	flt.pstate = fki_regs_pstate(regs);
	flt.x0 = fki_regs_reg(regs, 0);
	fki_regs_set_reg(regs, 1, 0x600d);
	fki_regs_set_pc(regs, flt.pc + 4);	/* go on after the instruction */
	return 0;
}

static long peek_fn(void *p)
{
	return ifacetest_peek((unsigned long)p);
}

static long undef_fn(void *p)
{
	return ifacetest_undef();
}

static void t_fault(void)
{
	unsigned long r;
	int err, again;

	err = fki_fault_register(0, PAGE_SIZE, fault_handler);
	again = fki_fault_register(0, PAGE_SIZE, fault_handler);
	CHECK(!err && again == -EBUSY, "fki_fault_register() = %d, a second one = %d", err, again);
	if (err)
		return;
	flt = (struct fault){ };
	r = in_foreign(peek_fn, (void *)0x10, NULL);
	CHECK(flt.calls == 1 && flt.far == 0x10 && flt.pc == (unsigned long)ifacetest_peek_insn && flt.x0 == 0x10 && ESR_ELx_EC(flt.esr) == ESR_ELx_EC_DABT_CUR,
	      "a load from address 0x10 in the registered range, inside fki_enter(): the handler was called %lu time(s) with far %#lx, esr %#lx, pc %s the load, x0 %#lx",
	      flt.calls, flt.far, flt.esr, flt.pc == (unsigned long)ifacetest_peek_insn ? "at" : "NOT at", flt.x0);
	CHECK((flt.pstate & (PSR_MODE_MASK | PSR_I_BIT)) == (PSR_MODE_EL1h | PSR_I_BIT), "fki_regs_pstate() = %#lx: EL1 with its interrupts off", flt.pstate);
	CHECK(r == 0x600d, "after it the code went on where fki_regs_set_pc() said, with the register fki_regs_set_reg() wrote: %#lx (to be 0x600d)", r);
	fki_fault_unregister();

	err = fki_fault_register((unsigned long)ifacetest_peek, (unsigned long)ifacetest_peek + 32, fault_handler);
	CHECK(!err, "fki_fault_unregister(), then fki_fault_register() for the range of the code = %d", err);
	if (err)
		return;
	flt = (struct fault){ };
	r = in_foreign(peek_fn, (void *)0x18, NULL);
	CHECK(flt.calls == 1 && flt.far == 0x18 && r == 0x600d, "a fault at an address outside the range, from code inside it: the handler was called %lu time(s), far %#lx", flt.calls, flt.far);
	fki_fault_unregister();
}

/* an exception that is not an abort, in the registered range (fki's probe of die(); run.sh checks that fki has it) */
static void t_undef(void)
{
	unsigned long r;
	int err;

	err = fki_fault_register((unsigned long)ifacetest_undef, (unsigned long)ifacetest_undef + 32, fault_handler);
	if (err) {
		CHECK(0, "fki_fault_register() = %d", err);
		return;
	}
	flt = (struct fault){ };
	r = in_foreign(undef_fn, NULL, NULL);
	CHECK(flt.calls == 1 && flt.far == 0 && flt.pc == (unsigned long)ifacetest_undef_insn && r == 0x600d,
	      "an undefined instruction in the registered range, inside fki_enter(): the handler was called %lu time(s) with far %#lx, esr %#lx, pc %s the instruction; the code went on with %#lx",
	      flt.calls, flt.far, flt.esr, flt.pc == (unsigned long)ifacetest_undef_insn ? "at" : "NOT at", r);
	fki_fault_unregister();
}

/* ---- the FP/SIMD registers (fpload.c loads this module with its own registers set, and looks at them after) ---- */

static long fp_fn(void *p)
{
	ifacetest_simd(0xee);
	fki_yield();					/* Linux runs, and the registers are the foreign kernel's again after */
	ifacetest_simd(0xdd);
	fki_idle_until(fki_counter() + div64_u64(fki_counter_frequency(), 500), 2 * NSEC_PER_MSEC, NULL);
	ifacetest_simd(0xcc);
	return 0;
}

static void t_fp(void)
{
	in_foreign(fp_fn, NULL, NULL);
	pr_info("ifacetest: inside fki_enter() every FP/SIMD register, FPSR and FPCR were written three times, with fki_yield() and fki_idle_until() between\n");
}

static const struct { const char *name; void (*fn)(void); } tests[] = {
	{ "enter", t_enter }, { "clock", t_clock }, { "idle", t_idle }, { "cpus", t_cpus }, { "net", t_net }, { "netwake", t_netwake },
	{ "file", t_file }, { "blob", t_blob }, { "mem", t_mem }, { "carve", t_carve }, { "fault", t_fault }, { "undef", t_undef }, { "fp", t_fp },
};

static int __init ifacetest_init(void)
{
	int i;

	fbuf = kmalloc(2048, GFP_KERNEL);
	stack_mem = vzalloc(4 * THREAD_SIZE);
	if (!fbuf || !stack_mem) {
		kfree(fbuf); vfree(stack_mem);
		return -ENOMEM;
	}
	/* (as Linux's stacks and XNU's lie: the exception entry takes a set bit THREAD_SHIFT of sp for an overflow) */
	stack_top = PTR_ALIGN(stack_mem, 2 * THREAD_SIZE) + THREAD_SIZE;
	for (i = 0; i < ARRAY_SIZE(tests); i++)
		if (!strcmp(t, tests[i].name))
			break;
	if (i < ARRAY_SIZE(tests))
		tests[i].fn();
	else
		CHECK(0, "no such test: t=%s", t);
	pr_info("ifacetest: %s: %s\n", t, bad ? "FAIL" : "ok");
	kfree(fbuf); vfree(stack_mem);
	return -EAGAIN;
}
module_init(ifacetest_init);
MODULE_LICENSE("GPL");

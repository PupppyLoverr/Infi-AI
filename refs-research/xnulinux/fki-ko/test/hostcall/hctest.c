// SPDX-License-Identifier: GPL-2.0-only
/*
 * fki_hostcall() called the way xnu.ko calls it for the hosted kernel (LINUX64_CALL_HOSTCALL in xnu-ko/xnu_main.c):
 * from inside fki_enter(), no preemption and no interrupts, Linux's own keys loaded again for the host code as
 * xnu_world_upcall does. What each call returns and how long it took goes to the kernel log; hcserve.c is the service
 * at the other end of /dev/xnu-hostcall and run.sh pairs each test here (t=) with the way the service behaves.
 * Loading the module is the test; it never stays loaded.
 */
#include <linux/ktime.h>
#include <linux/module.h>
#include <linux/string.h>
#include <linux/vmalloc.h>
#include "fki.h"

/* the DRM bridge's largest message: sizeof(struct xnudrm_hdr) + XNUDRM_MAX_MSG (gpu/xnu-drm/xnudrm_proto.h) */
#define BRIDGE_MAX ((8UL << 20) + 40)

static char *t = "";
module_param(t, charp, 0);

#define NO_PAC __attribute__((target("branch-protection=none")))
static struct fki_pac_keys foreign = { .sctlr_en = ~0ULL };
struct call { const void *req; unsigned long len; void *reply; unsigned long max; };

static NO_PAC long hostcall(struct call *c)
{
	struct fki_pac_keys saved, mine, tmp;
	unsigned long cookie;
	long r;

	cookie = fki_enter(&foreign, &saved);
	fki_pac_swap(&saved, &mine);		/* Linux's keys while host code runs */
	r = fki_hostcall(c->req, c->len, c->reply, c->max);
	fki_pac_swap(&mine, &tmp);
	fki_leave(cookie, &saved);
	return r;
}

static unsigned char *req, *reply;
static int bad;

/* hcserve's reply to a request: its first 8 bytes (the tag), then every byte inverted */
static int is_reply_to(u64 tag, unsigned long len, long r)
{
	unsigned long i;

	if (r != (long)len || memcmp(reply, &tag, 8))
		return 0;
	for (i = 8; i < len; i++)
		if (reply[i] != (unsigned char)~req[i])
			return 0;
	return 1;
}

/* one call: len bytes with this tag, room for max bytes of reply. Returns fki_hostcall()'s result; *ms: how long it took */
static long call(u64 tag, unsigned long len, unsigned long max, s64 *ms)
{
	struct call c = { req, len, reply, max };
	unsigned long i;
	ktime_t t0;
	long r;

	memcpy(req, &tag, 8);
	for (i = 8; i < len; i++)
		req[i] = (unsigned char)(tag + i * 7);
	memset(reply, 0, 16);
	t0 = ktime_get();
	r = hostcall(&c);
	*ms = ktime_ms_delta(ktime_get(), t0);
	return r;
}

/* a call that must come back with its own reply within limit_ms */
static void answered(const char *what, u64 tag, unsigned long len, s64 limit_ms)
{
	s64 ms;
	long r = call(tag, len, BRIDGE_MAX, &ms);
	int ok = is_reply_to(tag, len, r) && ms <= limit_ms;

	pr_info("hctest: %s: %s: a request of %lu bytes: fki_hostcall() = %ld after %lld ms%s\n", ok ? "ok" : "FAIL", what, len, r, ms,
		r < 0 ? "" : is_reply_to(tag, len, r) ? ", the reply to this request" : ", NOT the reply to this request");
	bad |= !ok;
}

/* a call that must fail with this error, after at least min_ms and at most max_ms */
static void fails(const char *what, u64 tag, unsigned long len, unsigned long max, long want, s64 min_ms, s64 max_ms)
{
	s64 ms;
	long r = call(tag, len, max, &ms);
	int ok = r == want && ms >= min_ms && ms <= max_ms;

	pr_info("hctest: %s: %s: a request of %lu bytes: fki_hostcall() = %ld after %lld ms (to be %ld)\n", ok ? "ok" : "FAIL", what, len, r, ms, want);
	bad |= !ok;
}

static int __init hctest_init(void)
{
	s64 ms;
	long r;

	req = vmalloc(BRIDGE_MAX);
	reply = vmalloc(BRIDGE_MAX);
	if (!req || !reply) {
		vfree(req); vfree(reply);
		return -ENOMEM;
	}
	if (!strcmp(t, "nosvc")) {
		fails("no service has the device open", 0x1001, 64, 64, -ENODEV, 0, 1000);
	} else if (!strcmp(t, "echo")) {
		answered("the service answers", 0x2001, 8, 2000);
		answered("the service answers", 0x2002, 4096, 2000);
		answered("the service answers", 0x2003, 1UL << 20, 2000);
		fails("a reply longer than the caller's buffer", 0x2004, 4096, 100, -EMSGSIZE, 0, 2000);
		answered("the call after it", 0x2005, 64, 2000);
	} else if (!strcmp(t, "one")) {
		answered("the service's first try fails, its second is right", 0x3001, 4096, 2000);
		answered("the call after it", 0x3002, 64, 2000);
	} else if (!strcmp(t, "close")) {
		fails("the service closes the device with the call unanswered", 0x4001, 64, 64, -ENODEV, 0, 2000);
	} else if (!strcmp(t, "bigreq")) {
		answered("the DRM bridge's largest message (8 MiB and its 40-byte header), both ways", 0x5001, BRIDGE_MAX, 5000);
	} else if (!strcmp(t, "bigreply")) {
		r = call(0x6001, 64, BRIDGE_MAX, &ms);
		pr_info("hctest: %s: a reply of the DRM bridge's largest size (%lu bytes): fki_hostcall() = %ld after %lld ms\n",
			r == BRIDGE_MAX && ms <= 5000 ? "ok" : "FAIL", BRIDGE_MAX, r, ms);
		bad |= !(r == BRIDGE_MAX && ms <= 5000);
	} else if (!strcmp(t, "timeout")) {
		/* 120 s, and what the kernel's timers add to a wait that long (up to an eighth) */
		fails("the service reads the request and does not answer", 0x7001, 64, 64, -ETIMEDOUT, 119000, 136000);
		answered("the call after it (the service looks again 125 s after the first)", 0x7002, 64, 10000);
	} else if (!strcmp(t, "slowwrite")) {
		/* the service's write() of the reply to A is still inside the kernel when A's time is up */
		r = call(0xA00A, 64, 64, &ms);
		pr_info("hctest: %s: call A, whose reply the service is still writing after 120 s: fki_hostcall() = %ld after %lld ms%s\n",
			r == -ETIMEDOUT || is_reply_to(0xA00A, 64, r) ? "ok" : "FAIL", r, ms,
			r < 0 ? "" : is_reply_to(0xA00A, 64, r) ? ", the reply to A" : ", NOT the reply to A");
		bad |= !(r == -ETIMEDOUT || is_reply_to(0xA00A, 64, r));
		r = call(0xB00B, 64, 64, &ms);
		pr_info("hctest: %s: call B, made next: fki_hostcall() = %ld after %lld ms%s\n", is_reply_to(0xB00B, 64, r) ? "ok" : "FAIL", r, ms,
			r < 0 ? "" : is_reply_to(0xB00B, 64, r) ? ", the reply to B" : !memcmp(reply, (u64[]){ 0xA00A }, 8) ? ", the reply to A" : ", not the reply to B");
		bad |= !is_reply_to(0xB00B, 64, r);
	} else {
		pr_info("hctest: no such test: t=%s\n", t);
		bad = 1;
	}
	pr_info("hctest: %s: %s\n", t, bad ? "FAIL" : "ok");
	vfree(req); vfree(reply);
	return -EAGAIN;
}
module_init(hctest_init);
MODULE_LICENSE("GPL");

// SPDX-License-Identifier: APSL-2.0
/*
 * xnu.ko: hosts an XNU kernel image inside Linux. Talks to Linux only
 * through FKI (fki.h); <linux/module.h> provides the module ABI.
 *
 * mode=1: load, slide and sign the image, call two XNU functions (a smoke test).
 * mode=2: a bare kernel image; mode=3: a kernel collection. Hand XNU boot args
 *        and an ADT and run arm_init in the XNU world; XNU calls into this
 *        module through linux64_hook (the interface: linux64_abi.h).
 */
#include <linux/module.h>
#include "../fki-ko/fki.h"
#include "xrt.h"
#include "macho.h"
#include "adt.h"
#include "world.h"
#include "../xnu-patches/include/linux64_abi.h"

static char *image = "xnu/kernel.macho";
module_param(image, charp, 0444);
static int mode = 2;
module_param(mode, int, 0444);
static int ram_mb = 256;
module_param(ram_mb, int, 0444);
static int kmax;	/* end the run after this many XNU kprintf lines (0: no limit) */
module_param(kmax, int, 0444);
static int kcount;
static int idle_max;	/* end the run after this many XNU idle rounds (0: never) */
module_param(idle_max, int, 0444);
static int quiet_console = 1;	/* drop the known high-volume console and kprintf lines (see quiet_line) */
module_param(quiet_console, int, 0444);
static int quiet_line(const char *l, unsigned n)
{
	static const char *const pfx[] = { "com.apple.xpc.launchd|", "kmutil: AppleImage4", "cryptexd: AppleImage4", "ACMTRM", "ACM:",
		"AppleCredentialManagerUserClient", "hfs: set hardlink bit", "Couldn't alloc class", "  dynamic: platformVersion" };
	for (unsigned i = 0; i < sizeof(pfx) / sizeof(pfx[0]); i++) {
		unsigned k = 0;
		while (pfx[i][k] && k < n && l[k] == pfx[i][k]) k++;
		if (!pfx[i][k]) return 1;
	}
	for (unsigned k = 0; k + 18 <= n; k++)	/* "... triggered unnest of range ..." */
		if (!x_strncmp(l + k, "triggered unnest", 16)) return 1;
	return 0;
}
/*
 * XNU's CPUs: Linux CPUs 0 .. cpus-1 (XNU's CPU 0 is Linux CPU 0, where xnu.ko's loading task runs). No more than
 * XNU's own tables hold (LINUX64_MAX_CPUS): XNU faulted registering a CPU beyond them (12 asked of a kernel built for
 * 8: ml_processor_register read the ninth entry of an eight-entry table).
 */
#define XNU_MAX_CPUS LINUX64_MAX_CPUS
static int cpus = 1;
module_param(cpus, int, 0444);
static struct xnu_world worlds[XNU_MAX_CPUS];		/* worlds[0] is xnu_W's twin: see xnu_worlds */
struct xnu_world *xnu_worlds[LINUX64_CPU_SLOTS];	/* by CPU slot (world.S finds its CPU's save area here) */
static signed char linux_cpu_of_slot[LINUX64_CPU_SLOTS];
static unsigned long long cpu_mpidr[XNU_MAX_CPUS];
static void *cpu_thread[XNU_MAX_CPUS];
static volatile int cpu_running[XNU_MAX_CPUS];
#define STOP_PARKED_MASK 0x0000ffffu
#define STOP_ACTIVE_SHIFT 16
#define STOP_ACTIVE_MASK 0x7fff0000u
#define STOP_ACTIVE_ONE (1u << STOP_ACTIVE_SHIFT)
#define STOP_COMMIT 0x80000000u
static volatile unsigned stop_state = STOP_ACTIVE_ONE;
static inline int stop_cas(unsigned old, unsigned new);
static void stop_commit(void);
static int stop_park(int me);
static u64_t a_cpu_entry;			/* XNU's linux64_cpu_entry */
static int el0_mode;				/* nonzero when host reports FKI_CAP_EL0 */
struct cpu_slot_hot {
	volatile int in_xnu_state, in_call;
	unsigned long upcalls, last_call;
	void *utcb;
	void *utcb_as;
	u64_t utcb_thread;
	u64_t utcb_rop_pid, utcb_jop_pid;
	int utcb_fpu_live, utcb_disable_jop;
} __attribute__((aligned(64)));
static struct cpu_slot_hot cpu_hot[LINUX64_CPU_SLOTS];
static inline unsigned this_slot(void)
{
	if (el0_mode) {
		unsigned long id;
		__asm__ volatile("mrs %0, tpidrro_el0" : "=r"(id));
		return (unsigned)(id & (LINUX64_CPU_SLOTS - 1));
	}
	unsigned long m;
	__asm__ volatile("mrs %0, mpidr_el1" : "=r"(m));
	return LINUX64_CPU_SLOT(m);
}
/* Writing 1 ends the run at XNU's next call into the host; insmod returns and the module can be removed */
static int stop_run;
module_param(stop_run, int, 0644);
static int assert_fatal;	/* 1: a failed XNU assert() ends the run; 0: report it and let XNU continue */
module_param(assert_fatal, int, 0444);
static int xo_compat;		/* 1: opt-in fallback relaxing --x to r-x when host lacks FKI_CAP_EXEC_ONLY */
module_param(xo_compat, int, 0444);
/*
 * EL0 KTCB lacks EL1 MRS emulation, so el0_patch_image replaces 2 ID_AA64ISAR0/PFR0_EL1
 * instructions in com.apple.kec.corecrypto, invalidating its static __TEXT HMAC.
 * By default (el0_fips_sha256=""), _fipspost_post_integrity is NEVER patched so the real
 * integrity failure path stays intact. Only when the operator supplies the exact 64-hex
 * SHA-256 of the unpatched com.apple.kec.corecrypto loaded segments is the integrity check
 * updated after verifying the digest and that exactly those 2 ID-reg patches were applied.
 */
static char *el0_fips_sha256 = "";
module_param(el0_fips_sha256, charp, 0444);

/* fki_pac_swap failed in an EL0 upcall: said here (XNU's keys may still be loaded); world.S then ends the run */
__attribute__((target("branch-protection=none")))
void xnu_world_pac_abort_el0(int err);
__attribute__((target("branch-protection=none")))
void xnu_world_pac_abort_el0(int err)
{
	static const char msg[] = "xnu: fki_pac_swap failed in EL0 upcall; ending the run\n";

	(void)err;
	fki_log_alert(msg, sizeof(msg) - 1);
}

static struct xnu_image img;
static struct fki_pac_keys xnu_user_keys;	/* separate PAC key domain for EL0 user contexts */
static struct fki_region l2page;
static unsigned long long *window_slot, *window_l1;
static unsigned char window_owned[512];
static u64_t diag_va[6];
#define XNU_L1_FIRST 384	/* VM_MIN_KERNEL_ADDRESS 0xffffffe000000000 */
#define XNU_L1_LAST 495	/* VM_MAX_KERNEL_ADDRESS 0xfffffffbffffffff */
static void *blob;
/* A root RAM disk (firmware blob): placed in XNU's DRAM after the ADT, /chosen/memory-map RAMDisk, boot-arg rd=md0. */
static char *ramdisk;
/* Extra XNU boot arguments, appended to the built-in command line */
static char *bootargs;
module_param(bootargs, charp, 0444);
module_param(ramdisk, charp, 0444);
static const void *rd_data; static unsigned long rd_size; static void *rd_blob;
/* The static trust cache a bootloader passes (raw trust cache module, e.g. the payload of the OS's StaticTrustCache.img4) */
static char *trustcache;
module_param(trustcache, charp, 0444);
static const void *tc_data; static unsigned long tc_size; static void *tc_blob;
#define TC_SEG_SIZE (tc_size ? tc_size + 8 : 0)	/* trust_cache_offsets_t { num_caches = 1, offsets[1] } + the module */
/*
 * The display's framebuffer (boot_args Video, 32 bpp): reserved in the boot data like the RAM disk, so XNU never hands its pages
 * out; XnuDisplay (kexts/XnuDisplay) copies each frame WindowServer swaps into it. fb_width=0 boots without one. A host thread
 * writes it to fb_dump (raw BGRA, fb_width x fb_height) every fb_dump_ms milliseconds when both are set.
 */
static int fb_width = 1280, fb_height = 800;
module_param(fb_width, int, 0444);
module_param(fb_height, int, 0444);
static char *fb_dump;
module_param(fb_dump, charp, 0444);
static int fb_dump_ms = 1000;
module_param(fb_dump_ms, int, 0444);
static u64_t fb_off;	/* the framebuffer's offset in the contiguous region (its Linux address: img.region.va + fb_off) */
#define FB_SIZE (fb_width > 0 && fb_height > 0 ? (u64_t)fb_width * fb_height * 4 : 0)
#define BOOT_EXTRA (((rd_size + 0x3fff) & ~0x3fffULL) + ((TC_SEG_SIZE + 0x3fff) & ~0x3fffULL) + ((FB_SIZE + 0x3fff) & ~0x3fffULL))
/* A root disk backed by a host file (an HFS+ image): XNU's memdev md0 in host mode (LINUX64_CALL_RAMDISK_SIZE, _RW). */
static char *hostdisk;
static int hostdisk_rw;
module_param(hostdisk, charp, 0444);
module_param(hostdisk_rw, int, 0444);
static void *hd_file; static fki_u64 hd_size;
/* Console input for XNU's /dev/console: bytes appended to this host file are typed in (LINUX64_CALL_CONSOLE_READ) */
static char *console_in;
module_param(console_in, charp, 0444);
static void *ci_file; static fki_u64 ci_off;
/*
 * Input events for XnuInput (kexts/XnuInput): a host file of 8-byte records {u16 type, u16 code, s32 value} as Linux's evdev
 * (tools/hosted/xnu-input.py appends them); LINUX64_CALL_INPUT_READ reads the whole records written since the load.
 */
static char *input_in;
module_param(input_in, charp, 0444);
static char *keystore;		/* the hosted keystore's file (XnuEAN's EAN blobs): read and written through LINUX64_CALL_KEYSTORE_RW */
module_param(keystore, charp, 0444);
static void *ks_file;
/* The hosted system's block disk (kexts/XnuDisk: IOKit media, e.g. an APFS container), read-write (LINUX64_CALL_DISK_SIZE, _RW) */
static char *disk;
module_param(disk, charp, 0444);
static void *dk_file; static fki_u64 dk_size;
/*
 * What iBoot tells an APFS boot in /chosen: the system volume's UUID (boot-uuid; boot-arg rd=uuid finds the root by it) and
 * the volume group's folder in Preboot (apfs-preboot-uuid: kern.apfsprebootuuid, from which dyld's libignition grafts the
 * boot cryptexes, the OS cryptex with the dyld shared cache, before launchd runs).
 */
static char *boot_uuid, *preboot_uuid;
module_param(boot_uuid, charp, 0444);
module_param(preboot_uuid, charp, 0444);
/*
 * root_uuid: the root volume itself, by its APFS volume UUID, as /chosen root-matching (IOFindBSDRoot takes it before
 * boot-uuid): BSD disk numbers follow probe order and change from boot to boot, and the boot-uuid route waits on APFS to
 * publish boot-uuid-media from its volume group tree, which a disk that never booted on a Mac does not have yet.
 */
static char *root_uuid;
module_param(root_uuid, charp, 0444);
static char root_matching[256];
/* One of the three as XNU takes it (uuid_parse: kern.bootuuid, kern.apfsprebootuuid): 8-4-4-4-12 hexadecimal digits; an empty one is none */
static int uuid_ok(const char *s)
{
	int i;

	if (!s || !s[0]) return 1;
	for (i = 0; s[i] && i < 36; i++) {
		char c = s[i];
		if (i == 8 || i == 13 || i == 18 || i == 23 ? c != '-' : !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')))
			return 0;
	}
	return i == 36 && !s[i];
}
static void *in_file; static fki_u64 in_off;
struct xnu_world xnu_W;

static void logs(const char *s) { fki_log(s, x_strlen(s)); }
static void logs_alert(const char *s) { fki_log_alert(s, x_strlen(s)); }
static void logkv(const char *k, u64_t v)
{
	char buf[128]; char *p = buf;
	while (*k && p < buf + sizeof(buf) - 20) *p++ = *k++;	/* the value's " 0x", 16 digits and NUL always fit: a longer key is cut */
	*p++ = ' '; *p++ = '0'; *p++ = 'x';
	for (int i = 60; i >= 0; i -= 4) *p++ = "0123456789abcdef"[(v >> i) & 15];
	*p = 0;
	logs(buf);
}

/* ---- M1 ---- */
typedef unsigned long (*strlen_fn)(const char *);
typedef unsigned int (*crc32_fn)(unsigned int, const void *, unsigned long);

static int run_m1(void)
{
	struct fki_pac_keys saved; unsigned long c;
	c = fki_enter(&xnu_W.xnu_keys, &saved);
	int n = macho_rebase(&img);
	fki_leave(c, &saved);
	logkv("xnu: threaded fixups applied", (u64_t)n);
	if (n < 0) return n;
	u64_t v = macho_sym(&img, "_version"), sl = macho_sym(&img, "_strlen"), cr = macho_sym(&img, "_crc32");
	if (!v || !sl || !cr) { logs("xnu: symbols missing"); return -2; }
	c = fki_enter(&xnu_W.xnu_keys, &saved);
	unsigned long len = ((strlen_fn)sl)((const char *)v);
	unsigned int crc = ((crc32_fn)cr)(0, (const void *)v, len);
	fki_leave(c, &saved);
	logs("xnu: XNU code ran inside Linux; XNU's version string:");
	fki_log((const char *)v, len);
	logkv("xnu: strlen(version) computed by XNU", len);
	logkv("xnu: crc32(version) computed by XNU", crc);
	return 0;
}

/* ---- M2a ---- */
#define XNU_FAKE_PHYS 0x800000000ULL	/* XNU's view of the region's first byte */
#define BOOT_LINE 1024

struct boot_args_v2 {			/* pexpert/arm64/boot.h layout */
	unsigned short Revision, Version;
	u64_t virtBase, physBase, memSize, topOfKernelData;
	struct { u64_t v_baseAddr, v_display, v_rowBytes, v_width, v_height, v_depth; } Video;
	unsigned int machineType;
	void *deviceTreeP;
	unsigned int deviceTreeLength;
	char CommandLine[BOOT_LINE];
	u64_t bootFlags, memSizeActual;
};

/*
 * The machine's model (the device tree root's "model", hw.model): user space takes the product type from it
 * (MobileGestalt ProductType), and apsd aborts without one, which holds up login (loginwindow's MDM pre-login check-in
 * waits on apsd). It must have the form of a Mac model identifier.
 */
static char *model = "VirtualMac2,1";	/* Apple's own virtual Mac's model identifier */
module_param(model, charp, 0444);
/* Names for System Information: the chip (/product product-soc-name) and the GPU (the sgx node's "model"). */
static char *soc_name;
module_param(soc_name, charp, 0444);
/* XNU's CPUs (bit c: XNU CPU c) of cluster-type 'E' in the device tree: the host's efficiency cores (e.g. the N1X's
 * Cortex-A725s), so the AMP scheduler keeps foreground threads on the 'P' ones. 0: every CPU 'P'. */
static unsigned long ecpus;
module_param(ecpus, ulong, 0444);
static char *gpu_model;
module_param(gpu_model, charp, 0444);

/* the compressor's limit is in VM pages: 6 GB either way */
static const char *cmdline = "debug=0x8 -v -noprogress serial=0 startup_debug=1";
static const char *cmdline_4k = " vm_compression_limit=1572864";
static const char *cmdline_16k = " vm_compression_limit=393216 linux64_vm16k=1";
/* vm16k=1 (the default): XNU's VM pages are 16 KB, as on a Mac, over the host's 4 KB translation granule (XNU's
 * PAGE_RATIO 4): a quarter of the page faults of 4 KB pages; vm16k=0 gives 4 KB VM pages */
static int vm16k = 1;
module_param(vm16k, int, 0444);


/* Walk XNU's kernel tables (inside its contiguous DRAM) for va and log the descriptors. */
static unsigned long long *tbl_va(u64_t desc)
{
	u64_t pa = desc & 0x0000fffffffff000ULL;
	if (pa < img.region.pa || pa >= img.region.pa + img.region.size) return 0;
	return (unsigned long long *)((char *)img.region.va + (pa - img.region.pa));
}

static void walk_log(const char *what, u64_t va, const unsigned long long *l1)
{
	u64_t d1 = l1[(va >> 30) & 511], d2 = 0, d3 = 0;
	logs(what);
	logkv("xnu:   va", va);
	logkv("xnu:   L1", d1);
	if ((d1 & 3) == 3) {
		unsigned long long *t2 = tbl_va(d1);
		if (!t2) { logs("xnu:   L2 table outside XNU DRAM"); return; }
		d2 = t2[(va >> 21) & 511];
		logkv("xnu:   L2", d2);
		if ((d2 & 3) == 3) {
			unsigned long long *t3 = tbl_va(d2);
			if (!t3) { logs("xnu:   L3 table outside XNU DRAM"); return; }
			d3 = t3[(va >> 12) & 511];
			logkv("xnu:   L3", d3);
		}
	}
}

/*
 * Kernel page faults on XNU addresses, taken in Linux's vectors while XNU runs (XNU's keys and stack live), go to
 * XNU's own linux64_kernel_fault (vm_fault on the kernel map). in_xnu is 0 while host code runs (upcalls, monitor).
 */
#define in_xnu cpu_hot[this_slot()].in_xnu_state
static unsigned long (*xnu_kfault)(unsigned long far, unsigned long esr);
static unsigned long (*xnu_kfault_recover)(unsigned long pc, unsigned long far, unsigned long esr);
static unsigned long kfaults;
static unsigned long kfault_moved;	/* kernel faults whose vm_fault came back on another CPU (the monitor's moved=) */

#define XNU_VA_LO 0xffffffe000000000ULL
#define XNU_VA_HI 0xfffffffc00000000ULL
/*
 * XNU takes less memory than half of that window, the image and the boot data included (56 GiB): from there its
 * arm_vm_init panics ("Unsupported memory configuration").
 */
#define XNU_MEM_MAX ((XNU_VA_HI - XNU_VA_LO) / 2)
/*
 * A kernel fault XNU's vm_fault should resolve is not resolved here, inside Linux's exception handler (a kprobe: no
 * sleeping, no rescheduling, and a nested fault in it is a missed kprobe and an oops), because vm_fault may page in
 * from disk (a file I/O upcall that lets Linux schedule) or block the thread. The exception instead returns into
 * xnu_fault_tramp (world.S) with every register as it was; the trampoline calls xnu_fault_deferred in XNU's own
 * context, then returns (eret) to the faulting instruction, to the copyio recovery path, or to the abort.
 */
/* each CPU's: the fault handler and the trampoline it returns into run on the same CPU, with nothing between them */
static struct { unsigned long far, esr, pc, pstate; } xnu_deferred_slot[LINUX64_CPU_SLOTS];
#define xnu_deferred xnu_deferred_slot[this_slot()]
extern void xnu_fault_tramp(void);
extern void xnu_world_fault_abort(void);

/*
 * No shadow call stack here: vm_fault may block, and the thread may come back on another CPU, whose Linux task's shadow
 * stack x18 then points into (the return address pushed on the first CPU's would be popped from the second's). XNU never
 * saves x18 per thread, so the register follows the CPU: what this calls pushes and pops on the current CPU's.
 */
unsigned long xnu_fault_deferred(unsigned long *frame) __attribute__((no_sanitize("shadow-call-stack")));
unsigned long xnu_fault_deferred(unsigned long *frame)
{
	unsigned long far = xnu_deferred.far, esr = xnu_deferred.esr, pc = xnu_deferred.pc;
	/* per CPU: with one global record, two CPUs faulting at once reset each other's count, and a CPU looping on one
	 * address never reached the every-64 yield below (it held its Linux CPU, and Linux with it, for good) */
	static unsigned long last_far_slot[LINUX64_CPU_SLOTS], repeats_slot[LINUX64_CPU_SLOTS];
	unsigned s0 = this_slot();
	unsigned long repeats = (far == last_far_slot[s0]) ? repeats_slot[s0] + 1 : 0;
	repeats_slot[s0] = repeats;
	last_far_slot[s0] = far;

	frame[31] = xnu_deferred.pstate;	/* the SPSR the trampoline returns with */
	/*
	 * The same address faulting again is normal (libmalloc's MADV_FREE_REUSABLE clears a page's referenced and
	 * modified state, so each reuse takes one fault). Only report it, rarely, when it looks endless.
	 */
	long kr = xnu_kfault(far, esr);
	if (this_slot() != s0) {
		/*
		 * vm_fault blocked and the thread came back on another CPU. xnu_fault_tramp's eret restores every register
		 * of the faulting context from frame, x18 included: that would be the first CPU's shadow stack pointer, and
		 * from then on this CPU's upcalls push and pop on the first CPU's Linux task's shadow stack while that CPU
		 * uses it too (corrupted shadow stacks, FPAC in whatever Linux code runs next). x18 follows the CPU.
		 */
		unsigned long x18;
		__asm__ volatile("mov %0, x18" : "=r"(x18));
		frame[18] = x18;
		kfault_moved++;
	}
	if (kr == 0) {
		if (repeats && (repeats & (repeats - 1)) == 0 && repeats >= 1024) {
			logkv("xnu: one address keeps taking resolved kernel faults, repeats", repeats);
			logkv("xnu:   far", far);
			logkv("xnu:   esr", esr);
			logkv("xnu:   pc ", pc);
			logkv("xnu:   lr ", frame[30]);
		}
		/* a fault loop runs with interrupts masked: let Linux take its interrupts now and then (RCU, network) */
		if ((repeats & 63) == 63)
			((void (*)(unsigned long, unsigned long, unsigned long))xnu_world_upcall)(LINUX64_CALL_PREEMPTED, 0, 0);
		return pc;
	}
	/* inside copyin/copyout: the routine's recovery path returns EFAULT to its caller, as XNU's own abort handler does */
	if (xnu_kfault_recover) {
		unsigned long rpc = xnu_kfault_recover(pc, far, esr);
		if (rpc) {
			frame[0] = 14;	/* EFAULT */
			return rpc;
		}
	}
	in_xnu = 0;
	logs("xnu: XNU took a fault vm_fault cannot resolve; ending the run");
	logkv("xnu:   pc ", pc);
	logkv("xnu:   lr ", frame[30]);
	logkv("xnu:   far", far);
	logkv("xnu:   esr", esr);
	return (unsigned long)xnu_world_fault_abort;
}

static int xnu_fault(unsigned long far, unsigned long esr, void *regs)
{
	if (!in_xnu)
		return 1;
	unsigned long fpc = fki_regs_pc(regs);
	if (far && ((far >= XNU_VA_LO && far < XNU_VA_HI) || far < 0x0001000000000000ULL) && xnu_kfault &&
	    fpc >= XNU_VA_LO && fpc < XNU_VA_HI) {
		kfaults++;
		xnu_deferred.far = far;
		xnu_deferred.esr = esr;
		xnu_deferred.pc = fpc;
		xnu_deferred.pstate = fki_regs_pstate(regs);
		fki_regs_set_pc(regs, (unsigned long)xnu_fault_tramp);
		return 0;
	}
	/*
	 * An XNU fault nothing resolves (a bug, or a NULL dereference): report it and end the run instead of letting Linux
	 * oops with XNU's keys and stack live (which takes the machine with it). The exception returns into
	 * xnu_world_fault_abort, which puts Linux's keys back and leaves through xnu_world_abort_mm.
	 */
	unsigned long pc = fki_regs_pc(regs);
	if (pc < XNU_VA_LO || pc >= XNU_VA_HI)
		return 1;
	/* inside copyin/copyout: the routine's recovery path returns EFAULT to its caller, as XNU's own abort handler does */
	/*
	 * brk #0xff00: XNU's UBSan minimal soft trap (signed overflow). XNU's own exception handler notes it
	 * (ubsan_handle_brk_trap) and resumes after the brk; do the same.
	 */
	if ((esr >> 26) == 0x3c && (esr & 0xffff) == 0xff00) {
		static unsigned long ubsan_traps;
		if (ubsan_traps++ < 8)
			logkv("xnu: UBSan soft trap (signed overflow), resuming; pc", pc);
		fki_regs_set_pc(regs, pc + 4);
		return 0;
	}
	if (xnu_kfault_recover) {
		unsigned long rpc = xnu_kfault_recover(pc, far, esr);
		if (rpc) {
			fki_regs_set_reg(regs, 0, 14);	/* EFAULT */
			fki_regs_set_pc(regs, rpc);
			return 0;
		}
	}
	in_xnu = 0;
	logs("xnu: XNU took a fault it cannot handle; ending the run");
	logkv("xnu:   pc ", pc);
	logkv("xnu:   lr ", fki_regs_lr(regs));
	logkv("xnu:   far", far);
	logkv("xnu:   esr", esr);
	logkv("xnu:   CPU slot", this_slot());
	logkv("xnu:   x0 ", fki_regs_reg(regs, 0));
	logkv("xnu:   x1 ", fki_regs_reg(regs, 1));
	{	/* XNU's frame-pointer chain (return addresses with the kernel's all-ones top in place of their PAC bits) */
		unsigned long *fp = (unsigned long *)fki_regs_reg(regs, 29);
		for (int i = 0; i < 12 && fp && !((unsigned long)fp & 15) && (unsigned long)fp >= XNU_VA_LO &&
		    (unsigned long)fp < XNU_VA_HI; i++) {
			fki_u64 w0, w1;
			/* a frame that is not mapped ends the walk (XNU's stack may be what went wrong): read directly it
			 * faulted here, in the exception handler with XNU's keys loaded, the oops this path is there to avoid */
			if (fki_high_read_u64x2((unsigned long)fp, &w0, &w1) != 0)
				break;
			logkv("xnu:   frame", w1 | 0xffffff8000000000ul);
			if ((unsigned long *)w0 <= fp) break;
			fp = (unsigned long *)w0;
		}
	}
	fki_regs_set_pc(regs, (unsigned long)xnu_world_fault_abort);
	return 0;
}

/* A secondary CPU's host thread: its own world, entered at linux64_cpu_entry(cpu_data); returns when the run ends */
static unsigned long cpu_start_arg[XNU_MAX_CPUS];
static int park_secondaries;	/* diagnostics: 1 keeps secondary CPUs out of XNU */
module_param(park_secondaries, int, 0444);
static int cpu_fn(void *arg)
{
	int c = (int)(unsigned long)arg;

	fki_thread_wait(&cpu_start_arg[c]);	/* until XNU starts this CPU (LINUX64_CALL_CPU_START) */
	if (!cpu_start_arg[c])
		return 0;			/* the run ended first: told to stop */
	logkv("xnu: XNU CPU starts on Linux CPU", (u64_t)c);
	if (park_secondaries) {	/* diagnostics: the CPU is asked for but never enters XNU */
		logkv("xnu: secondary CPU parked (park_secondaries)", (u64_t)c);
		for (;;) {
			unsigned s = stop_state;
			if ((s & STOP_COMMIT) || ((s & STOP_ACTIVE_MASK) <= STOP_ACTIVE_ONE))
				break;
			if (stop_cas(s, s - STOP_ACTIVE_ONE))
				break;
		}
		cpu_running[c] = 0;
		while (!fki_thread_should_stop())
			fki_msleep(10);
		return 0;
	}
	if (READ_ONCE(stop_run) && stop_park(c)) {
		cpu_running[c] = 0;
		while (!fki_thread_should_stop())
			fki_msleep(10);
		return 0;
	}
	in_xnu = 1;
	long r = el0_mode ? xnu_world_call_el0(a_cpu_entry, cpu_start_arg[c]) : xnu_world_call(a_cpu_entry, cpu_start_arg[c]);
	in_xnu = 0;
	if (!r) logkv("xnu: linux64_cpu_entry returned (unexpected) on CPU", (u64_t)c);
	stop_run = 1;			/* one CPU's run ending (a fatal fault, a stop) ends all of them */
	stop_commit();
	cpu_running[c] = 0;
	while (!fki_thread_should_stop())	/* kthread_stop() from the module's exit wants the thread alive */
		fki_msleep(10);
	return 0;
}

/*
 * Where the time goes: host-counter ticks spent in each kind of call into the host (the rest of a CPU's time is XNU
 * running, user or kernel). The monitor prints the share of each over its last interval (perf=1).
 */
#define ACCT_N 64
static unsigned long acct_ticks_slot[LINUX64_CPU_SLOTS][ACCT_N], acct_calls_slot[LINUX64_CPU_SLOTS][ACCT_N], acct_disk_slot[LINUX64_CPU_SLOTS];
static int perf;
module_param(perf, int, 0644);
static volatile const unsigned long *xnu_el0_stat;	/* XNU's linux64_el0_stat[cpu][count, ticks][class] */
static inline unsigned acct_bucket(unsigned long call) { return call >= 800 && call < 800 + ACCT_N - 1 ? call - 800 : ACCT_N - 1; }
static inline void acct_add(unsigned long call, u64_t ticks)
{
	unsigned b = acct_bucket(call), s = this_slot();	/* per CPU: no atomics (and none of the compiler's out-of-line ones) */
	acct_ticks_slot[s][b] += ticks;
	acct_calls_slot[s][b]++;
}

_Static_assert(sizeof(struct fki_utcb_regs) == 36 * sizeof(fki_u64), "fki_utcb_regs size");
_Static_assert(__builtin_offsetof(struct linux64_utcb_req, sp) - __builtin_offsetof(struct linux64_utcb_req, x) ==
	       __builtin_offsetof(struct fki_utcb_regs, sp), "utcb_req sp offset");
_Static_assert(__builtin_offsetof(struct linux64_utcb_req, pc) - __builtin_offsetof(struct linux64_utcb_req, x) ==
	       __builtin_offsetof(struct fki_utcb_regs, pc), "utcb_req pc offset");
_Static_assert(__builtin_offsetof(struct linux64_utcb_req, pstate) - __builtin_offsetof(struct linux64_utcb_req, x) ==
	       __builtin_offsetof(struct fki_utcb_regs, pstate), "utcb_req pstate offset");
_Static_assert(__builtin_offsetof(struct linux64_utcb_req, tpidr) - __builtin_offsetof(struct linux64_utcb_req, x) ==
	       __builtin_offsetof(struct fki_utcb_regs, tpidr), "utcb_req tpidr offset");
_Static_assert(__builtin_offsetof(struct linux64_utcb_req, tpidrro) - __builtin_offsetof(struct linux64_utcb_req, x) ==
	       __builtin_offsetof(struct fki_utcb_regs, tpidrro), "utcb_req tpidrro offset");

/*
 * Stopping the run: the CPUs leave XNU together. A CPU still inside XNU could otherwise wait for good, interrupts masked,
 * for one that had already left. Each parks briefly at its host calls and kicks the others, so that one idle or in user
 * space comes to a host call too; the run is abandoned once all active CPUs are parked at once.
 * stop_state layout: [15:0] parked count, [30:16] active CPU count, [31] STOP_COMMIT.
 */
static inline int stop_cas(unsigned old, unsigned new)
{
	unsigned cur, fail;
	__asm__ volatile(
		"1:	ldaxr	%w[cur], [%[p]]\n"
		"	cmp	%w[cur], %w[old]\n"
		"	b.ne	2f\n"
		"	stlxr	%w[fail], %w[new], [%[p]]\n"
		"	cbnz	%w[fail], 1b\n"
		"2:"
		: [cur] "=&r"(cur), [fail] "=&r"(fail)
		: [p] "r"(&stop_state), [old] "r"(old), [new] "r"(new)
		: "cc", "memory");
	return cur == old;
}
static void stop_commit(void)
{
	unsigned s;
	do s = stop_state; while (!(s & STOP_COMMIT) && !stop_cas(s, s | STOP_COMMIT));
}
static int stop_park(int me)	/* 1: the run is abandoned, do not return to XNU */
{
	unsigned s, last_active;
	u64_t end;

	do {
		s = stop_state;
		if (s & STOP_COMMIT) return 1;
	} while (!stop_cas(s, s + 1));
	last_active = (s & STOP_ACTIVE_MASK) >> STOP_ACTIVE_SHIFT;
	for (int c = 0; c < cpus; c++)
		if (c != me && (c == 0 || cpu_running[c]))
			fki_kick_cpu(c);
	end = fki_counter() + fki_counter_frequency() / 500;	/* 2 ms */
	for (;;) {
		unsigned parked, active;

		s = stop_state;
		if (s & STOP_COMMIT) return 1;
		parked = s & STOP_PARKED_MASK;
		active = (s & STOP_ACTIVE_MASK) >> STOP_ACTIVE_SHIFT;
		if (active != last_active) {
			last_active = active;
			for (int c = 0; c < cpus; c++)
				if (c != me && (c == 0 || cpu_running[c]))
					fki_kick_cpu(c);
		}
		if (parked >= active) { stop_cas(s, s | STOP_COMMIT); continue; }
		if (fki_counter() > end && stop_cas(s, s - 1)) return 0;
	}
}

/* XNU calls into the host: enum linux64_call (linux64_abi.h); a non-zero return ends the run */
static long xnu_hook_body(unsigned long call, unsigned long a1, unsigned long a2);
long xnu_hook(unsigned long call, unsigned long a1, unsigned long a2)
{
	unsigned in_s = this_slot();	/* the thread is bound to its CPU while in the host */
	struct cpu_slot_hot *hot = &cpu_hot[in_s];
	hot->upcalls++;
	hot->last_call = call;
	hot->in_xnu_state = 0;
	if (*(volatile int *)&stop_run && stop_park(linux_cpu_of_slot[in_s])) {	/* the host asked the run to end (scripts/xnulinux-boot.sh stop): XNU is abandoned here */
		logs("xnu: run stopped by the host (stop_run)");
		return 1;
	}
	u64_t t0 = 0;
	int do_perf = unlikely(READ_ONCE(perf));
	if (do_perf) {
		/* debugging: an XNU stack Linux's overflow check would reject (bit 14 of sp set) */
		static int told;
		unsigned long sp;
		__asm__ volatile("mov %0, sp" : "=r"(sp));
		if ((sp & 0x4000) && told < 3) {
			told++;
			logkv("xnu: upcall on an XNU stack with sp bit 14 set, sp", sp);
			logkv("xnu:   call", call);
		}
		t0 = fki_counter();
	}
	hot->in_call = 1;
	long r = xnu_hook_body(call, a1, a2);
	hot->in_call = 0;
	if (do_perf)
		acct_add(call, fki_counter() - t0);
	if (r)
		stop_commit();		/* this CPU ends the run by itself (a panic, a failed assert) */
	else if (*(volatile int *)&stop_run && stop_park(linux_cpu_of_slot[in_s])) {	/* back from a long call when the stop was asked */
		logs("xnu: run stopped by the host (stop_run)");
		r = 1;
	}
	if (!r) hot->in_xnu_state = 1;
	return r;
}

static void derive_user_pac_keys(u64_t rop_pid, u64_t jop_pid, int djop,
				 struct fki_pac_keys *out)
{
	struct fki_pac_keys k = xnu_user_keys;
	u64_t jmix = jop_pid * 0x9e3779b97f4a7c15ULL;
	u64_t rmix = rop_pid * 0x9e3779b97f4a7c15ULL;
	k.ia[0] ^= jop_pid; k.ia[1] ^= jmix;
	k.da[0] ^= jop_pid; k.da[1] ^= jmix;
	k.ga[0] ^= jop_pid; k.ga[1] ^= jmix;
	k.ib[0] ^= rop_pid; k.ib[1] ^= rmix;
	k.db[0] ^= rop_pid; k.db[1] ^= rmix;
	if (djop)	/* arm64 (not arm64e): all four keys off (EnIA, EnIB, EnDA, EnDB), as linux64_el0_run does at EL1 */
		k.sctlr_en &= ~((1ULL << 31) | (1ULL << 30) | (1ULL << 27) | (1ULL << 13));
	*out = k;
}

static __always_inline u64_t xnu_user_pac_exec(u64_t val, u64_t disc,
					       unsigned int key, unsigned int is_auth)
{
	if (!is_auth) {
		u64_t out = val;
		if (key == LINUX64_PAC_KEY_IA)
			__asm__ volatile(".arch armv8.3-a\n pacia %0, %1" : "+r"(out) : "r"(disc) : "memory");
		else
			__asm__ volatile(".arch armv8.3-a\n pacda %0, %1" : "+r"(out) : "r"(disc) : "memory");
		return out;
	}
	/*
	 * Authenticate without trapping under FEAT_FPAC: strip the PAC, re-sign
	 * under the active user JOP key, and return the stripped pointer only on
	 * an exact match (otherwise return an ARMv8.3-A A-key error-tagged ptr).
	 */
	u64_t stripped = val;
	if (key == LINUX64_PAC_KEY_IA)
		__asm__ volatile(".arch armv8.3-a\n xpaci %0" : "+r"(stripped) : : "memory");
	else
		__asm__ volatile(".arch armv8.3-a\n xpacd %0" : "+r"(stripped) : : "memory");
	u64_t expected = stripped;
	if (key == LINUX64_PAC_KEY_IA)
		__asm__ volatile(".arch armv8.3-a\n pacia %0, %1" : "+r"(expected) : "r"(disc) : "memory");
	else
		__asm__ volatile(".arch armv8.3-a\n pacda %0, %1" : "+r"(expected) : "r"(disc) : "memory");
	if (expected == val)
		return stripped;
	return (stripped & ~(3ULL << 61)) | (1ULL << 61);
}

static long xnu_hook_body(unsigned long call, unsigned long a1, unsigned long a2)
{
	if (call == LINUX64_CALL_USER_PAC_OP) {
		struct linux64_user_pac_req *q = (struct linux64_user_pac_req *)a1;
		if (!q)
			return 0;
		struct fki_pac_keys uk, saved, tmp;
		derive_user_pac_keys(0, q->jop_key, 0, &uk);
		int perr = fki_pac_swap(&uk, &saved);
		if (perr != 0) {
			q->err = perr;
			return 0;
		}
		q->result = xnu_user_pac_exec(q->value, q->discriminator, q->key, q->is_auth);
		q->err = 0;
		int rerr = fki_pac_swap(&saved, &tmp);
		if (rerr != 0) {
			logs("xnu: failed to restore kernel PAC keys");
			return 1;
		}
		return 0;
	}
	if (call == LINUX64_CALL_UTCB_RUN || call == LINUX64_CALL_UTCB_RESUME) {
		struct linux64_utcb_req *q = (struct linux64_utcb_req *)a1;
		if (!q || !q->as_handle)
			return 0;
		unsigned s = this_slot();
		struct cpu_slot_hot *hot = &cpu_hot[s];
		void *as = (void *)(unsigned long)q->as_handle;
		void *utcb = hot->utcb;
		int djop = q->disable_jop ? 1 : 0;
		if (call == LINUX64_CALL_UTCB_RESUME &&
		    likely(utcb && hot->utcb_thread == q->thread_id && !q->fpu_dirty &&
			   hot->utcb_as == as && hot->utcb_disable_jop == djop &&
			   hot->utcb_rop_pid == q->rop_pid && hot->utcb_jop_pid == q->jop_pid)) {
			struct fki_utcb_stop stop;
			int err = fki_utcb_resume(utcb, (struct fki_utcb_regs *)&q->x[0], &stop);
			q->result = err;
			if (err == 0) {
				hot->utcb_fpu_live = 1;
				q->stop_kind = stop.kind;
				q->fault_addr = stop.fault_addr;
				q->fault_ip = stop.fault_ip;
				q->esr = stop.esr;
			}
			return 0;
		}
		if (call == LINUX64_CALL_UTCB_RESUME) {
			/*
			 * The registers of a resume are the ones the host's context holds, not q's (those are from the
			 * thread's last full stop): when the context is no longer this thread's as it stopped, they are
			 * gone, and running q's would take the thread back to that stop. XNU keeps the two together
			 * (linux64_el0_utcb_entry, no preemption in between): say so instead.
			 */
			logs_alert("xnu: UTCB_RESUME for a thread whose registers the CPU's user context no longer holds");
			q->result = -116;	/* -ESTALE */
			return 0;
		}
		if (!utcb) {
			utcb = fki_utcb_create(as);
			if (!utcb) {
				q->result = -12;
				return 0;
			}
			hot->utcb = utcb;
			hot->utcb_as = as;
			hot->utcb_thread = 0;
			hot->utcb_fpu_live = 0;
			hot->utcb_disable_jop = -1;
			hot->utcb_rop_pid = ~0ULL;
			hot->utcb_jop_pid = ~0ULL;
		} else if (hot->utcb_as != as) {
			int aerr = fki_utcb_set_as(utcb, as);
			if (aerr != 0) {
				q->result = aerr;
				return 0;
			}
			hot->utcb_as = as;
		}
		if (hot->utcb_disable_jop != djop ||
		    hot->utcb_rop_pid != q->rop_pid ||
		    hot->utcb_jop_pid != q->jop_pid) {
			struct fki_pac_keys k;
			derive_user_pac_keys(q->rop_pid, q->jop_pid, djop, &k);
			int perr = fki_utcb_set_pac(utcb, &k);
			if (perr != 0) {
				q->result = perr;
				return 0;
			}
			hot->utcb_disable_jop = djop;
			hot->utcb_rop_pid = q->rop_pid;
			hot->utcb_jop_pid = q->jop_pid;
		}
		if (q->fpu_dirty || hot->utcb_thread != q->thread_id) {
			if (!q->neon_ptr) {
				q->result = -22;
				return 0;
			}
			int ferr = fki_utcb_fpu_write(utcb, (const void *)(unsigned long)q->neon_ptr, 520);
			if (ferr != 0) {
				q->result = ferr;
				return 0;
			}
			hot->utcb_thread = q->thread_id;
			hot->utcb_fpu_live = 0;
			q->fpu_dirty = 0;
		}
		struct fki_utcb_stop stop;
		int err = fki_utcb_run(utcb, (struct fki_utcb_regs *)&q->x[0], &stop);
		q->result = err;
		if (err == 0) {
			hot->utcb_fpu_live = 1;
			q->stop_kind = stop.kind;
			q->fault_addr = stop.fault_addr;
			q->fault_ip = stop.fault_ip;
			q->esr = stop.esr;
		}
		return 0;
	}
	if (call == LINUX64_CALL_UTCB_SYNC_REGS) {
		struct linux64_utcb_req *q = (struct linux64_utcb_req *)a1;
		if (!q)
			return 0;
		unsigned s = this_slot();
		struct cpu_slot_hot *hot = &cpu_hot[s];
		if (unlikely(!hot->utcb)) {
			q->result = -22;
			return 0;
		}
		q->result = fki_utcb_sync_regs(hot->utcb, (struct fki_utcb_regs *)&q->x[0]);
		return 0;
	}
	if (call == LINUX64_CALL_UTCB_FPU_SYNC) {
		struct linux64_fpu_sync_req *q = (struct linux64_fpu_sync_req *)a1;
		if (!q)
			return 0;
		unsigned s = this_slot();
		struct cpu_slot_hot *hot = &cpu_hot[s];
		q->result = 0;
		if ((q->flags & LINUX64_FPU_FLUSH) &&
		    hot->utcb && hot->utcb_thread == q->thread_id && hot->utcb_fpu_live) {
			if (!q->neon_ptr) {
				q->result = -22;
				return 0;
			}
			int ferr = fki_utcb_fpu_read(hot->utcb, (void *)(unsigned long)q->neon_ptr, 520);
			if (ferr != 0) {
				q->result = ferr;
				return 0;
			}
			hot->utcb_fpu_live = 0;
		}
		if ((q->flags & LINUX64_FPU_INVALIDATE) && hot->utcb_thread == q->thread_id) {
			hot->utcb_fpu_live = 0;
			hot->utcb_thread = 0;
		}
		return 0;
	}
	if (call == LINUX64_CALL_AS_MAP) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q)
			q->result = fki_as_map((void *)(unsigned long)q->as_handle, (unsigned long)q->va,
					       q->pa, (unsigned long)q->size, (int)q->prot);
		return 0;
	}
	if (call == LINUX64_CALL_AS_UNMAP) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q) {
			fki_as_unmap((void *)(unsigned long)q->as_handle, (unsigned long)q->va,
				     (unsigned long)q->size);
			q->result = 0;
		}
		return 0;
	}
	if (call == LINUX64_CALL_AS_FLUSH) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q) {
			fki_as_flush((void *)(unsigned long)q->as_handle);
			q->result = 0;
		}
		return 0;
	}
	if (call == LINUX64_CALL_ICACHE_FLUSH) {
		fki_icache_flush(a1, a2);
		return 0;
	}
	if (call == LINUX64_CALL_CONSOLE_READ) {		/* console input: new bytes of console_in */
		struct linux64_buf_req *q = (struct linux64_buf_req *)a1;
		if (!q) return 0;
		q->result = 0;
		if (ci_file) {
			long n = fki_file_rw(ci_file, 0, q->buf, (unsigned long)q->len, ci_off);
			if (n > 0) { ci_off += n; q->result = n; }
		}
		return 0;
	}
	if (call == LINUX64_CALL_INPUT_READ) {		/* input events: whole 8-byte records of input_in, into q->buf */
		struct linux64_buf_req *q = (struct linux64_buf_req *)a1;
		if (!q) return 0;
		q->result = 0;
		if (in_file && q->len >= 8) {
			long n = fki_file_rw(in_file, 0, q->buf, (unsigned long)(q->len & ~7ULL), in_off);
			if (n > 0) { n &= ~7L; in_off += n; q->result = n; }
		}
		return 0;
	}
	if (call == LINUX64_CALL_NET_SEND || call == LINUX64_CALL_NET_RECV) {	/* XNU's Ethernet link: one frame each way */
		struct linux64_buf_req *q = (struct linux64_buf_req *)a1;
		if (!q) return 0;
		q->result = call == LINUX64_CALL_NET_SEND ? fki_net_send(q->buf, (unsigned long)q->len) : fki_net_recv(q->buf, (unsigned long)q->len);
		return 0;
	}
	if (call == LINUX64_CALL_WALLCLOCK) {		/* the host's wall clock for XNU's calendar: a1 -> u64[2] {seconds, nanoseconds} UTC */
		if (a1) fki_realtime(&((fki_u64 *)a1)[0], &((fki_u64 *)a1)[1]);
		return 0;
	}
	if (call == LINUX64_CALL_RANDOM) {	/* a1: a2 bytes from Linux's generator */
		if (a1) fki_random((void *)a1, a2);
		return 0;
	}
	if (call == LINUX64_CALL_HOSTCALL) {		/* host call: q->result = the reply's length or -errno */
		struct linux64_hostcall_req *q = (struct linux64_hostcall_req *)a1;
		if (q) q->result = fki_hostcall(q->req, (unsigned long)q->len, q->reply, (unsigned long)q->max);
		return 0;
	}
	if (call == LINUX64_CALL_DISK_SIZE) {		/* the block disk's size in bytes into *a1, 0 when there is none */
		if (a1) *(u64_t *)a1 = dk_file ? dk_size : 0;
		return 0;
	}
	if (call == LINUX64_CALL_DISK_RW) {		/* block disk I/O: q->result = bytes moved or -errno */
		struct linux64_rw_req *q = (struct linux64_rw_req *)a1;
		if (q) q->result = dk_file ? fki_file_rw(dk_file, (int)q->write, q->buf, (unsigned long)q->len, q->off) : -6;
		if (q && q->result > 0) acct_disk_slot[this_slot()] += (unsigned long)q->result;
		return 0;
	}
	if (call == LINUX64_CALL_KEYSTORE_RW) {		/* the keystore file: q->result = bytes moved or -errno */
		struct linux64_rw_req *q = (struct linux64_rw_req *)a1;
		if (!q) return 0;
		q->result = ks_file ? fki_file_rw(ks_file, (int)q->write, q->buf, (unsigned long)q->len, q->off) : -6;
		return 0;
	}
	if (call == LINUX64_CALL_RAMDISK_SIZE) {		/* host disk size in bytes into *a1, 0 when there is none */
		if (a1) *(u64_t *)a1 = hd_file ? hd_size : 0;
		return 0;
	}
	if (call == LINUX64_CALL_RAMDISK_RW) {		/* host disk I/O: q->result = bytes moved or -errno */
		struct linux64_rw_req *q = (struct linux64_rw_req *)a1;
		if (!q) return 0;
		if (!hd_file) q->result = -6;
		else if (q->write && !hostdisk_rw) q->result = -30;
		else q->result = fki_file_rw(hd_file, (int)q->write, q->buf, (unsigned long)q->len, q->off);
		return 0;
	}
	if (call == LINUX64_CALL_CONSOLE_WRITE) {		/* XNU console bytes (/dev/console): line-buffered into the host log */
		static char line[1000]; static unsigned n;
		const char *b = (const char *)a1; unsigned long i;
		for (i = 0; b && i < a2; i++) {
			char c = b[i];
			if (c == '\r') continue;
			if (c != '\n') line[n++] = c;
			if (c == '\n' || n == sizeof(line) - 1) {
				static char m[1024]; unsigned j = 0, k;
				const char *p = "darwin console: ";
				while (*p) m[j++] = *p++;
				for (k = 0; k < n; k++) m[j++] = line[k];
				m[j] = 0;
				/* quiet_console: launchd's and AppleImage4's chatter (thousands of lines a second) stays out */
				if (!(quiet_console && quiet_line(line, n)))
					logs(m);
				n = 0;
			}
		}
		/* a prompt (a write that ends in "# ", "$ " or "> " without a newline): show it now */
		if (n >= 2 && line[n - 1] == ' ' && (line[n - 2] == '#' || line[n - 2] == '$' || line[n - 2] == '>')) {
			static char m[1024]; unsigned j = 0, k;
			const char *p = "darwin console: ";
			while (*p) m[j++] = *p++;
			for (k = 0; k < n; k++) m[j++] = line[k];
			m[j] = 0; n = 0;
			logs(m);
		}
		return 0;
	}
	if (call == LINUX64_CALL_KPRINTF) {		/* XNU kprintf */
		char m[260]; const char *f = (const char *)a1; unsigned i = 0, j = 0;
		m[j++] = 'x'; m[j++] = 'n'; m[j++] = 'u'; m[j++] = ']'; m[j++] = ' ';
		while (f && f[i] && j < sizeof(m) - 1) { if (f[i] != '\n') m[j++] = f[i]; i++; }
		m[j] = 0;
		/* the per-entry startup trace is thousands of lines: count it, do not print it */
		if (!(f && !x_strncmp(f, "kernel_startup_initialize_upto[", 31)) && !(quiet_console && quiet_line(m + 5, j - 5)))
			logs(m);
		if (kmax && ++kcount >= kmax) { logs("xnu: kprintf budget reached"); return 1; }
		return 0;
	}
	if (call == LINUX64_CALL_KERNEL_TABLES && window_l1) {	/* install XNU's kernel L1 entries (the kernel VA range only) */
		/*
		 * XNU's L1 table (cpu_tte) is read through the bootstrap window, and installing XNU's entry for the window's
		 * own slot replaces that window: XNU's tables need not map cpu_tte at this stage (L3 0 here), so the rest of
		 * the table is read from a copy taken first, never through the slot being replaced (it faulted in Linux's
		 * context, an oops, whenever the TLB no longer held the bootstrap entry).
		 */
		static unsigned long long tte[512];
		x_memcpy(tte, (const void *)a1, sizeof(tte));
		walk_log("xnu: XNU table walk for arm_init (text)", diag_va[0], tte);
		walk_log("xnu: XNU table walk for the Mach-O header", diag_va[1], tte);
		walk_log("xnu: XNU table walk for boot args", diag_va[2], tte);
		walk_log("xnu: XNU table walk for the cpu_tte page", a1, tte);
		int n = 0, last = -1;
		for (int i = XNU_L1_FIRST; i <= XNU_L1_LAST; i++) {
			if (!(tte[i] & 1)) continue;
			if (window_l1[i] && &window_l1[i] != window_slot) { logkv("xnu: L1 slot in use by Linux, refusing:", i); return 1; }
			window_l1[i] = tte[i]; window_owned[i] = 1; n++; last = i;
		}
		if (last >= 0) fki_window_set(&window_l1[last], tte[last]);	/* barrier + TLB flush */
		logkv("xnu: XNU L1 entries installed", n);
		/* Touch XNU memory through XNU's own tables now (Linux context: a fault here is a readable oops). */
		logkv("xnu: probe text word", *(volatile unsigned int *)diag_va[0]);
		logkv("xnu: probe Mach-O magic", *(volatile unsigned int *)diag_va[1]);
		logkv("xnu: probe boot args Version", ((volatile struct boot_args_v2 *)diag_va[2])->Version);
		logkv("xnu: probe ADT word", *(volatile unsigned int *)diag_va[3]);
		if (diag_va[4]) { volatile u64_t *pc = (void *)diag_va[4]; u64_t v = *pc; *pc = v; logkv("xnu: probe port_crumb (rw)", v); }
		if (diag_va[5]) { unsigned long l = ((unsigned long (*)(const char *))diag_va[5])("xnulinux"); logkv("xnu: probe exec strlen through XNU tables", l); }
		logs("xnu: probes done");
	}
	if (call == LINUX64_CALL_PREEMPTED) {		/* XNU busy: a host IRQ preempted its user code; let Linux handle it */
		fki_yield();
		return 0;
	}
	if (call == LINUX64_CALL_IDLE) {		/* XNU idle: let Linux run until XNU's next timer deadline */
		static unsigned long idles;
		idles++;
		if ((idles & (idles - 1)) == 0) logkv("xnu: XNU idle, count", idles);
		if (!a1)		/* a timer is due: only let Linux take its interrupts */
			fki_yield();
		else {			/* sleep until XNU's next timer, at most 20 ms (XNU's devices are polled from its timers) */
			u64_t ns = a1 < 20000000ULL ? a1 : 20000000ULL;
			fki_idle_until(fki_counter() + ns * fki_counter_frequency() / 1000000000ULL, ns,
			    (const volatile unsigned int *)a2);
		}
		return idle_max && idles >= (unsigned long)idle_max;
	}
	if (call == LINUX64_CALL_ASSERT) {		/* XNU assert() failed */
		const struct linux64_assert_info *a = (const void *)a1;
		char m[240]; unsigned j = 0, i;
		const char *parts[3] = { "xnu: XNU assert failed: ", a->file, ": " };
		for (int p = 0; p < 3; p++)
			for (i = 0; parts[p] && parts[p][i] && j < sizeof(m) - 1; i++) m[j++] = parts[p][i];
		for (i = 0; a->expr && a->expr[i] && j < sizeof(m) - 1; i++) m[j++] = a->expr[i];
		m[j] = 0;
		logs(m);
		logkv("xnu:   at line", a->line);
		return assert_fatal;
	}
	if (call == LINUX64_CALL_USER_ENTER) {		/* the first entry into user space (linux64_el0_run follows) */
		logkv("xnu: XNU enters user space (EL0); user pc", a1);
		return 0;
	}
	if (call == LINUX64_CALL_HALT) {	/* the hosted system shut down: XNU would spin with this CPU's interrupts masked */
		logs(a1 ? "xnu: the hosted system asked for a restart: run ended" : "xnu: the hosted system halted: run ended");
		return 1;
	}
	if (call == LINUX64_CALL_PANIC) {
		char m[500]; const char *f = (const char *)a1; unsigned i = 0;
		while (f && f[i] && i < sizeof(m) - 1) { m[i] = f[i]; i++; }
		m[i] = 0;
		logs_alert("xnu: XNU panic:");
		logs_alert(m);
		logkv("xnu: panic caller", a2);
		if (diag_va[4])
			logkv("xnu: port_crumb", *(volatile u64_t *)diag_va[4]);
		{	/* the frame-pointer chain from here through XNU's frames (the hook runs on XNU's stack); return addresses
			 * with their pointer-authentication bits replaced by the kernel's all-ones top */
			unsigned long *fp = el0_mode ? (unsigned long *)xnu_worlds[this_slot()]->xnu_regs[10] : __builtin_frame_address(0);
			unsigned long fp_lo = el0_mode ? (unsigned long)XNU_VA_LO : 0xffffff8000000000UL;	/* EL0: a stack can be in the aperture, below the image */
			unsigned long fp_hi = 0xfffffffc00000000UL;
			for (i = 0; i < 24 && fp && !((unsigned long)fp & 15) &&
			     (unsigned long)fp >= fp_lo && (unsigned long)fp + 16 <= fp_hi; i++) {
				fki_u64 w0, w1;
				/* on either host: a frame that is not mapped ends the walk (a panic's stack may be damaged; read
				 * directly, Linux oopsed here with interrupts off in place of the panic's report) */
				if (fki_high_read_u64x2((unsigned long)fp, &w0, &w1) != 0)
					break;
				unsigned long lr = (unsigned long)w1 | 0xffffff8000000000ul;
				unsigned long *next = (unsigned long *)w0;
				logkv("xnu:   frame", lr);
				if (next <= fp || (unsigned long)next - (unsigned long)fp > 0x100000UL) break;
				fp = next;
			}
		}
		return 1;
	}
	if (call == LINUX64_CALL_CPU_START) {	/* start XNU's CPU of MPIDR a1 at linux64_cpu_entry(a2), asynchronously */
		int c = linux_cpu_of_slot[LINUX64_CPU_SLOT(a1)];
		/* its thread was made before XNU ran (an upcall runs with preemption off: no thread creation here) */
		if (c <= 0 || c >= cpus || !cpu_thread[c] || !a2 || cpu_start_arg[c]) {
			logkv("xnu: XNU asked to start a CPU it was not given, MPIDR", a1);
			return 0;
		}
		for (;;) {
			unsigned s;
			if (READ_ONCE(stop_run))
				return 0;
			s = stop_state;
			if (s & STOP_COMMIT)
				return 0;
			if (stop_cas(s, s + STOP_ACTIVE_ONE))
				break;
		}
		cpu_running[c] = 1;
		WRITE_ONCE(cpu_start_arg[c], a2);
		fki_thread_wake(cpu_thread[c]);
		return 0;
	}
	if (call == LINUX64_CALL_IPI) {		/* XNU's CPU of MPIDR a1 has an IPI pending: make it look */
		int c = linux_cpu_of_slot[LINUX64_CPU_SLOT(a1)];
		if (c >= 0)
			fki_kick_cpu(c);
		return 0;
	}
	if (call == LINUX64_CALL_KERNEL_TABLES)
		return 0;
	if (call == LINUX64_CALL_KMAP) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q)
			q->result = fki_high_map((unsigned long)q->va, q->pa, (unsigned long)q->size, (int)q->prot);
		return 0;
	}
	if (call == LINUX64_CALL_KUNMAP) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q) {
			fki_high_unmap((unsigned long)q->va, (unsigned long)q->size);
			q->result = 0;
		}
		return 0;
	}
	if (call == LINUX64_CALL_KFLUSH) {
		fki_high_flush();
		return 0;
	}
	if (call == LINUX64_CALL_KMAP_TXN) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q) {
			if (q->prot == LINUX64_KMAP_TXN_BEGIN) {
				void *txn = NULL;
				int err = fki_high_txn_begin((unsigned long)q->va, (unsigned long)q->size, &txn);
				q->as_handle = (u64_t)(unsigned long)txn;
				q->result = err;
			} else if (q->prot == LINUX64_KMAP_TXN_RESTORE) {
				fki_high_txn_restore((void *)(unsigned long)q->as_handle);
				q->result = 0;
			} else if (q->prot == LINUX64_KMAP_TXN_END) {
				fki_high_txn_end((void *)(unsigned long)q->as_handle);
				q->result = 0;
			} else {
				q->result = -22; /* -EINVAL */
			}
		}
		return 0;
	}
	if (call == LINUX64_CALL_AS_CREATE) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q) {
			q->as_handle = (u64_t)(unsigned long)fki_as_create();
			q->result = q->as_handle ? 0 : -12;
		}
		return 0;
	}
	if (call == LINUX64_CALL_AS_DESTROY) {
		struct linux64_map_req *q = (struct linux64_map_req *)a1;
		if (q && q->as_handle) {
			fki_as_destroy((void *)(unsigned long)q->as_handle);
			q->as_handle = 0;
			q->result = 0;
		}
		return 0;
	}
	{	/* a call this module does not know (an XNU built against a newer linux64_abi.h): say so once, go on */
		static unsigned long told;
		if (told != call) { told = call; logkv("xnu: unknown call from XNU", call); }
	}
	return 0;
}

/*
 * Boot handoff. The XNU "DRAM" region is described by (virtBase, physBase, size): XNU sees base_va as the
 * kernel virtual address of physBase. The image's vmlow sits at base_va + image_off.
 */
static struct boot_args_v2 *build_boot(unsigned char *base_va, u64_t phys, u64_t size, u64_t image_off, u64_t image_size)
{
	u64_t off_args = (image_off + image_size + 0x3fff) & ~0x3fffULL;
	struct boot_args_v2 *ba = (void *)(base_va + off_args);
	u64_t off_dt = off_args + 0x4000;
	struct adt a;
	static unsigned char seed[256];

	/* (ram_mb alone is below it: xnu_init) */
	if (size >= XNU_MEM_MAX) {
		logkv("xnu: ram_mb with the image and the boot data is more than XNU takes, by (bytes)", size - XNU_MEM_MAX + 1);
		return 0;
	}
	fki_random(seed, sizeof(seed));
	/*
	 * XNU identifies its CPUs by MPIDR: its CPU 0 runs here, bound to Linux CPU 0; CPUs 1 .. cpus-1 are Linux's of the
	 * same numbers, started when XNU asks (LINUX64_CALL_CPU_START). Their slots in the per-CPU arrays must differ.
	 */
	int berr = fki_bind_cpu(0);
	if (berr) { logkv("xnu: cannot bind to cpu0, err", (u64_t)berr); return 0; }
	if (cpus < 1) cpus = 1;
	if (cpus > XNU_MAX_CPUS) {
		logkv("xnu: more CPUs asked for than XNU is built for, using", (u64_t)XNU_MAX_CPUS);
		cpus = XNU_MAX_CPUS;
	}
	if (cpus > fki_num_cpus()) cpus = fki_num_cpus();
	for (int i = 0; i < LINUX64_CPU_SLOTS; i++) linux_cpu_of_slot[i] = -1;
	for (int c = 0; c < cpus; c++) {
		fki_u64 m = 0;
		if (fki_cpu_mpidr(c, &m) || linux_cpu_of_slot[LINUX64_CPU_SLOT(m)] >= 0) {
			logkv("xnu: no distinct MPIDR slot for Linux CPU, using CPUs below it:", (u64_t)c);
			cpus = c;
			break;
		}
		cpu_mpidr[c] = m & 0xffff;	/* XNU compares Aff1:Aff0 */
		linux_cpu_of_slot[LINUX64_CPU_SLOT(m)] = (signed char)c;
		xnu_worlds[LINUX64_CPU_SLOT(m)] = c ? &worlds[c] : &xnu_W;
	}
	u64_t mpidr = cpu_mpidr[0];
	logkv("xnu: running on MPIDR Aff1:Aff0", mpidr);
	logkv("xnu: XNU CPUs", (u64_t)cpus);

	adt_init(&a, base_va + off_dt, 0x40000);
	adt_node(&a, 3, 7);				/* / */
	adt_prop_str(&a, "name", "device-tree");
	{	/* the platform's own name, then the model: user space's product type (MobileGestalt) comes from this list */
		static char compat[96];
		static const char own[] = "xnulinux,linux64";
		unsigned n = sizeof(own), m = (unsigned)x_strlen(model);
		x_memcpy(compat, own, n);
		if (m && n + m + 1 <= sizeof(compat) && x_strcmp(model, "xnulinux")) {
			x_memcpy(compat + n, model, m + 1);
			n += m + 1;
		}
		adt_prop(&a, "compatible", compat, n);
	}
	adt_prop_str(&a, "model", model);
	if (root_uuid && x_strlen(root_uuid) == 36) {
		static const char pre[] = "<dict><key>IOProviderClass</key><string>IOMedia</string><key>IOPropertyMatch</key>"
		    "<dict><key>UUID</key><string>", post[] = "</string></dict></dict>";
		x_memcpy(root_matching, pre, sizeof(pre) - 1);
		x_memcpy(root_matching + sizeof(pre) - 1, root_uuid, 36);
		x_memcpy(root_matching + sizeof(pre) - 1 + 36, post, sizeof(post));
	}
	/* (an empty boot_uuid went in as it was, and XNU waited for the root volume of that UUID) */
	adt_node(&a, 22 + (boot_uuid && boot_uuid[0] ? 1 : 0) + (preboot_uuid && preboot_uuid[0] ? 1 : 0) + (root_matching[0] ? 1 : 0), 3);	/* /chosen */
	if (root_matching[0]) adt_prop_str(&a, "root-matching", root_matching);
	if (boot_uuid && boot_uuid[0]) adt_prop_str(&a, "boot-uuid", boot_uuid);
	if (preboot_uuid && preboot_uuid[0]) adt_prop_str(&a, "apfs-preboot-uuid", preboot_uuid);
	adt_prop_str(&a, "name", "chosen");
	adt_prop_u64(&a, "dram-base", phys);
	adt_prop_u64(&a, "dram-size", size);
	adt_prop(&a, "random-seed", seed, sizeof(seed));
	adt_prop_str(&a, "firmware-version", "xnulinux-m2");
	adt_prop_u32(&a, "debug-enabled", 1);
	{
		/* the SHA-384 of the boot manifest a bootloader would pass; there is none, so zeros (27's Image4 needs the property) */
		static const unsigned char zero48[48];
		adt_prop(&a, "boot-manifest-hash", zero48, sizeof(zero48));
	}
	{
		/* NVRAM as iBoot hands it over: the bank size and the image (blank: IODTNVRAM formats it as a CHRP store) */
		static unsigned char nvram[0x10000];
		adt_prop_u32(&a, "nvram-bank-size", sizeof(nvram));
		adt_prop_u32(&a, "nvram-bank-count", 1);
		adt_prop_u32(&a, "nvram-current-bank", 0);
		adt_prop(&a, "nvram-proxy-data", nvram, sizeof(nvram));
	}
	/*
	 * The identity a bootloader states for the AP, which Image4 reads: Apple's VM platform (chip 0xFE00, board 0x20)
	 * in an insecure, non-production mode, as a development VM boots.
	 */
	adt_prop_str(&a, "crypto-hash-method", "sha2-384");
	adt_prop_u32(&a, "chip-id", 0xfe00);
	adt_prop_u32(&a, "board-id", 0x20);
	adt_prop_u64(&a, "unique-chip-id", 0x0000584e554c4e58ULL);
	adt_prop_u32(&a, "chip-epoch", 0);
	adt_prop_u32(&a, "security-domain", 1);
	adt_prop_u32(&a, "certificate-production-status", 0);
	adt_prop_u32(&a, "certificate-security-mode", 0);
	adt_prop_u32(&a, "effective-production-status-ap", 0);
	adt_prop_u32(&a, "effective-security-mode-ap", 0);
	adt_prop_u32(&a, "mix-n-match-prevention-status", 0);
	u64_t off_rd = (off_dt + 0x40000 + 0x3fff) & ~0x3fffULL;	/* the RAM disk, after the ADT's room */
	u64_t off_tc = (off_rd + rd_size + 0x3fff) & ~0x3fffULL;	/* the trust cache segment, after the RAM disk */
	u64_t off_fb = (off_tc + TC_SEG_SIZE + 0x3fff) & ~0x3fffULL;	/* the framebuffer, after the trust cache */
	adt_node(&a, 1 + (rd_size ? 1 : 0) + (tc_size ? 1 : 0), 0);	/* /chosen/memory-map (no AuxKC) */
	adt_prop_str(&a, "name", "memory-map");
	if (rd_size) {
		/* in whole 4 KB pages, as XNU makes md0 of it (IOFindBSDRoot: size >> 12): a last page's part was cut off the disk */
		u64_t rd[2] = { phys + off_rd, (rd_size + 0xfff) & ~0xfffULL };
		adt_prop(&a, "RAMDisk", rd, sizeof(rd));
	}
	if (tc_size) {
		u64_t tc[2] = { phys + off_tc, TC_SEG_SIZE };	/* DTTrustCacheRange: load_static_trust_cache() */
		adt_prop(&a, "TrustCache", tc, sizeof(tc));
	}
	adt_node(&a, 1, 0);				/* /chosen/manifest-properties: AppleImage4 reads it (empty: no manifest) */
	adt_prop_str(&a, "name", "manifest-properties");
	adt_node(&a, 3, 0);				/* /chosen/asmb: the SIP configuration a bootloader passes (csr_bootstrap) */
	adt_prop_str(&a, "name", "asmb");
	adt_prop_u64(&a, "lp-sip0", 0x2);		/* CSR_ALLOW_UNTRUSTED_KEXTS, as DroidVM boots */
	adt_prop_u32(&a, "lp-sip1", 1);			/* CSR_ALLOW_UNAUTHENTICATED_ROOT */
	adt_node(&a, 1, cpus);				/* /cpus */
	adt_prop_str(&a, "name", "cpus");
	/*
	 * /cpus/cpu0, without device_type "cpu": that would match Apple's CPU driver (AppleARMCPU, IONameMatch "cpu"),
	 * which drives the AIC interrupts and IPIs of real hardware. While hosted, the CPU is Linux's.
	 */
	for (int c = 0; c < cpus; c++) {	/* compatible: kexts/XnuCPU registers each with XNU and starts it */
		static char cname[XNU_MAX_CPUS][8];
		cname[c][0] = 'c'; cname[c][1] = 'p'; cname[c][2] = 'u';
		if (c < 10) { cname[c][3] = '0' + c; cname[c][4] = 0; } else { cname[c][3] = '0' + c / 10; cname[c][4] = '0' + c % 10; cname[c][5] = 0; }
		adt_node(&a, 6, 0);
		adt_prop_str(&a, "name", cname[c]);
		adt_prop_str(&a, "compatible", "xnulinux,cpu");
		adt_prop_str(&a, "state", c ? "waiting" : "running");
		adt_prop_u32(&a, "reg", (unsigned)cpu_mpidr[c]);
		adt_prop_u64(&a, "timebase-frequency", fki_counter_frequency());
		adt_prop_u32(&a, "cluster-type", (ecpus >> c) & 1 ? 'E' : 'P');	/* the AMP scheduler wants E/M/P, not SMP */
	}
	adt_node(&a, 1, 0);				/* /defaults: XNU looks these up without checking */
	adt_prop_str(&a, "name", "defaults");
	adt_node(&a, 4 + (soc_name && soc_name[0] ? 1 : 0), 0);	/* /product */
	adt_prop_str(&a, "name", "product");
	/*
	 * MobileGestalt's product type in sandboxed processes: their profiles let them read only these two /product
	 * properties (apsd's: "sub-product-type", "unique-model"), and apsd aborts without a product type
	 */
	adt_prop_str(&a, "sub-product-type", model);
	adt_prop_str(&a, "unique-model", model);
	adt_prop_str(&a, "product-name", "xnulinux");
	if (soc_name && soc_name[0])
		adt_prop_str(&a, "product-soc-name", soc_name);
	/* /sgx: System Information describes the GPU from the first child of the service of this name (kexts/XnuCpuMetal) */
	adt_node(&a, 3 + (gpu_model && gpu_model[0] ? 1 : 0), 0);
	adt_prop_str(&a, "name", "sgx");
	adt_prop_str(&a, "device_type", "sgx");
	adt_prop_str(&a, "compatible", "xnulinux,gpu");
	if (gpu_model && gpu_model[0])
		adt_prop_str(&a, "model", gpu_model);
	adt_node(&a, 1, 0);				/* /options: NVRAM's node (AppleImage4 keeps its nonce seeds there) */
	adt_prop_str(&a, "name", "options");
	adt_node(&a, 3, FB_SIZE ? 1 : 0);		/* /arm-io: XNU needs a nonzero SoC base */
	adt_prop_str(&a, "name", "arm-io");
	adt_prop_str(&a, "device_type", "linux64-io");
	{ u64_t ranges[3] = { 0, 0x7000000000ULL, 0x10000000ULL }; adt_prop(&a, "ranges", ranges, sizeof(ranges)); }
	if (FB_SIZE) {
		/*
		 * /arm-io/gpu, compatible "paravirtualizedgraphics,gpu" as on Apple's VM: IOMobileFramebuffer.framework then waits
		 * for the framebuffers of the AppleParavirtGPU services (XnuDisplay; it matches IOResources, not this node, which
		 * no driver claims: /arm-io's device_type keeps Apple's ARM IO driver off). Without the node WindowServer counted
		 * no framebuffer and ran headless.
		 */
		adt_node(&a, 2, 0);
		adt_prop_str(&a, "name", "gpu");
		adt_prop_str(&a, "compatible", "paravirtualizedgraphics,gpu");
	}
	if (a.err) { logs("xnu: ADT overflow"); return 0; }
	u64_t dt_len = adt_size(&a);

	ba->Revision = 2; ba->Version = 2;
	ba->virtBase = (u64_t)base_va;
	ba->physBase = phys;
	ba->memSize = size;
	ba->memSizeActual = size;
	ba->topOfKernelData = phys + ((off_dt + dt_len + 0x3fff) & ~0x3fffULL);
	if (rd_size) {
		x_memcpy(base_va + off_rd, rd_data, rd_size);
		ba->topOfKernelData = phys + ((off_rd + rd_size + 0x3fff) & ~0x3fffULL);
		logkv("xnu: root RAM disk bytes", rd_size);
	}
	if (tc_size) {
		unsigned int hdr[2] = { 1, 8 };
		x_memcpy(base_va + off_tc, hdr, sizeof(hdr));
		x_memcpy(base_va + off_tc + 8, tc_data, tc_size);
		ba->topOfKernelData = phys + ((off_tc + TC_SEG_SIZE + 0x3fff) & ~0x3fffULL);
		logkv("xnu: static trust cache bytes", tc_size);
	}
	if (FB_SIZE) {
		unsigned int *px = (unsigned int *)(base_va + off_fb);
		for (u64_t i = 0; i < FB_SIZE / 4; i++)
			px[i] = 0xff000000;	/* opaque black until the first frame */
		ba->Video.v_baseAddr = phys + off_fb;
		ba->Video.v_display = 0;	/* no boot console drawn into it (the console is the serial line) */
		ba->Video.v_rowBytes = (u64_t)fb_width * 4;
		ba->Video.v_width = (u64_t)fb_width;
		ba->Video.v_height = (u64_t)fb_height;
		ba->Video.v_depth = 32;
		ba->topOfKernelData = phys + ((off_fb + FB_SIZE + 0x3fff) & ~0x3fffULL);
		fb_off = off_fb;
		logkv("xnu: framebuffer bytes", FB_SIZE);
	}
	ba->deviceTreeP = base_va + off_dt;
	ba->deviceTreeLength = (unsigned)dt_len;
	x_memcpy(ba->CommandLine, cmdline, x_strlen(cmdline) + 1);
	{
		const char *pg = vm16k ? cmdline_16k : cmdline_4k;
		x_memcpy(ba->CommandLine + x_strlen(ba->CommandLine), pg, x_strlen(pg) + 1);
	}
	if (rd_size || hd_file) {
		const char *rd = " rd=md0";
		x_memcpy(ba->CommandLine + x_strlen(ba->CommandLine), rd, x_strlen(rd) + 1);
	}
	{
		/*
		 * Apple's user space assumes a 24 MHz timebase (libswift_Concurrency: ticks = ns * 3 / 125) and XNU's machine
		 * timeouts are in those ticks: on another counter rate the kernel scales the counter (droidvm-timebase-hz).
		 * The debugger's cross-call gets 1 s, not 10 ms: a CPU may be inside a host call when asked to stop.
		 */
		const char *a = fki_counter_frequency() != 24000000 ?
		    " ml-timeout-debug-ack-scale=100 droidvm-timebase-hz=24000000" : " ml-timeout-debug-ack-scale=100";

		if (x_strlen(ba->CommandLine) + x_strlen(a) < sizeof(ba->CommandLine))
			x_memcpy(ba->CommandLine + x_strlen(ba->CommandLine), a, x_strlen(a) + 1);
	}
	if (bootargs) {
		char *e = ba->CommandLine + x_strlen(ba->CommandLine);

		/* (dropped whole and in silence before: the system came up without any of them and failed somewhere else) */
		if (x_strlen(ba->CommandLine) + 1 + x_strlen(bootargs) >= sizeof(ba->CommandLine)) {
			logkv("xnu: bootargs do not fit in XNU's command line; characters it has left",
			    sizeof(ba->CommandLine) - x_strlen(ba->CommandLine) - 2);
			return 0;
		}
		*e++ = ' ';
		x_memcpy(e, bootargs, x_strlen(bootargs) + 1);
	}
	logkv("xnu: boot args at", (u64_t)ba);
	logkv("xnu: ADT bytes", dt_len);
	logkv("xnu: topOfKernelData (phys)", ba->topOfKernelData);
	return ba;
}


/*
 * Monitor: XNU runs with interrupts masked on its CPU, so when it spins nothing reaches Linux. A helper
 * thread on another CPU prints XNU's debug counters (linux64_dbg, see the XNU patch) every 2 s.
 */
static volatile const unsigned long *xnu_dbg;
static void *monitor;
static int monitor_cpu = -2;	/* -2: the first Linux CPU XNU does not run on; -1: no monitor */
module_param(monitor_cpu, int, 0444);

static char *put_dec(char *p, unsigned long v)
{
	char t[24]; int n = 0;
	do { t[n++] = '0' + v % 10; v /= 10; } while (v);
	while (n) *p++ = t[--n];
	return p;
}

static void put_str(char **p, const char *s) { while (*s) *(*p)++ = *s++; }
static void put_hex(char **p, unsigned long v) { for (int i = 44; i >= 0; i -= 4) *(*p)++ = "0123456789abcdef"[(v >> i) & 15]; }

/* XNU's counters (enum linux64_counter) every 2 s for the first 3 minutes, then every 20 s */
/* writing 1 asks XNU for a dump of its threads at its next idle (/sys/module/xnu/parameters/dump_threads) */
static int dump_threads;
module_param(dump_threads, int, 0644);

/* perf=1: per 2 s, each call kind's share of the interval in permille of one CPU, its count, and disk bytes */
static void acct_report(void)
{
	static unsigned long last_t, last_ticks[ACCT_N], last_calls[ACCT_N], last_disk;
	unsigned long now = fki_counter(), dt = now - last_t, total = 0;
	char buf[800]; char *p = buf;
	if (!last_t) { last_t = now; return; }
	last_t = now;
	put_str(&p, "xnu: perf ms="); p = put_dec(p, dt * 1000 / fki_counter_frequency());
	for (unsigned b = 0; b < ACCT_N && p < buf + sizeof(buf) - 48; b++) {
		unsigned long t = 0, c = 0, d, n;
		for (unsigned s = 0; s < LINUX64_CPU_SLOTS; s++) { t += READ_ONCE(acct_ticks_slot[s][b]); c += READ_ONCE(acct_calls_slot[s][b]); }
		d = t - last_ticks[b]; n = c - last_calls[b];
		last_ticks[b] = t; last_calls[b] = c;
		if (!n) continue;
		total += d;
		put_str(&p, b == ACCT_N - 1 ? " other" : " c"); if (b != ACCT_N - 1) p = put_dec(p, 800 + b);
		put_str(&p, "="); p = put_dec(p, d * 1000 / dt);
		put_str(&p, "%o/"); p = put_dec(p, n);
	}
	put_str(&p, " host="); p = put_dec(p, total * 1000 / dt);
	if (xnu_el0_stat) {	/* XNU's user-space exceptions by class: count and permille of the interval until back in user space */
		static const char *cls[LINUX64_EL0_CLASSES] = { " svc=", " dabt=", " iabt=", " irq=", " other=" };
		static unsigned long last_n[LINUX64_EL0_CLASSES], last_t[LINUX64_EL0_CLASSES];
		for (unsigned c = 0; c < LINUX64_EL0_CLASSES; c++) {
			unsigned long n = 0, t = 0;
			for (unsigned s = 0; s < LINUX64_EL0_CPUS; s++) {
				n += READ_ONCE(xnu_el0_stat[(s * 2 + 0) * LINUX64_EL0_CLASSES + c]);
				t += READ_ONCE(xnu_el0_stat[(s * 2 + 1) * LINUX64_EL0_CLASSES + c]);
			}
			put_str(&p, cls[c]); p = put_dec(p, n - last_n[c]); put_str(&p, "/"); p = put_dec(p, (t - last_t[c]) * 1000 / dt); put_str(&p, "%o");
			last_n[c] = n; last_t[c] = t;
		}
	}
	unsigned long disk = 0;
	for (unsigned s = 0; s < LINUX64_CPU_SLOTS; s++) disk += READ_ONCE(acct_disk_slot[s]);
	put_str(&p, "%o diskKB="); p = put_dec(p, (disk - last_disk) >> 10);
	last_disk = disk;
	*p = 0;
	logs(buf);
}

static int monitor_fn(void *arg)
{
	static const struct { int i; const char *name; } dec[] = {
		{ LINUX64_CTR_IDLE_THREAD, " idle_thread=" }, { LINUX64_CTR_MACHINE_IDLE, " machine_idle=" },
		{ LINUX64_CTR_CPU_IDLE, " cpu_idle=" }, { LINUX64_CTR_HOST_IDLE, " host_idle=" }, { LINUX64_CTR_SWITCHES, " switches=" },
		{ LINUX64_CTR_LAST_CALL, " last_call=" }, { LINUX64_CTR_RTCLOCK, " rtclock=" }, { LINUX64_CTR_EL0_EXC, " el0exc=" },
	}, hex[] = {
		{ LINUX64_CTR_EL0_ESR, " esr=0x" }, { LINUX64_CTR_EL0_FAR, " far=0x" }, { LINUX64_CTR_EL0_PC, " pc=0x" },
		{ LINUX64_CTR_BAD_ESR, " bad_esr=0x" }, { LINUX64_CTR_THREAD, " thread=0x" },
	};
	int lines = 0;
	unsigned long ticks = 0;
	(void)arg;
	while (!fki_thread_should_stop()) {
		fki_msleep(2000);
		if (xnu_dbg && *(volatile int *)&dump_threads) {
			((volatile unsigned long *)xnu_dbg)[LINUX64_CTR_DUMP_REQ] = 1;
			dump_threads = 0;
		}
		{	/* a CPU in XNU that has not called the host for 2 s holds its Linux CPU with interrupts masked */
			static unsigned long seen[XNU_MAX_CPUS], still[XNU_MAX_CPUS];
			for (int c = 0; c < cpus && xnu_dbg; c++) {
				unsigned s = LINUX64_CPU_SLOT(cpu_mpidr[c]);
				if ((c && !cpu_running[c]) || cpu_hot[s].upcalls != seen[c]) { seen[c] = cpu_hot[s].upcalls; still[c] = 0; continue; }
				if (++still[c] == 1) {
					struct xnu_world *w = xnu_worlds[s];
					unsigned long fp = w->xnu_regs[10];
					{
						char ab[160]; char *q = ab;
						put_str(&q, "xnu: XNU CPU "); q = put_dec(q, (unsigned long)c);
						put_str(&q, " made no host call for 2 s (its Linux CPU held, interrupts masked); last call ");
						q = put_dec(q, cpu_hot[s].last_call); put_str(&q, cpu_hot[s].in_call ? " (inside it)" : " (back in XNU)");
						*q = 0;
						logs_alert(ab);
					}
					logkv("xnu: XNU CPU silent for 2 s (no host call), cpu", (u64_t)c);
					logkv("xnu:   its last host call", cpu_hot[s].last_call);
					logkv("xnu:   still inside it (1) or back in XNU (0)", cpu_hot[s].in_call);
					logkv("xnu:   lr at that call", w->xnu_regs[11]);
					for (int f = 0; f < 16 && fp >= XNU_VA_LO && fp < XNU_VA_HI && !(fp & 7); f++) {
						fki_u64 w0, w1;
						/* the frame pointer is from the CPU's last host call: that stack may be gone since */
						if (fki_high_read_u64x2(fp, &w0, &w1) != 0)
							break;
						unsigned long next = (unsigned long)w0;
						logkv("xnu:   frame", w1);
						if (next <= fp || next - fp > 0x100000UL) break;
						fp = next;
					}
				}
			}
		}
		if (perf && cpus > 1) {	/* each XNU CPU's host calls so far and the last one's number */
			static char cb[16 + XNU_MAX_CPUS * 42];	/* room for every CPU: two 64-bit decimals and two separators each */
			char *q = cb;
			put_str(&q, "xnu: calls");
			for (int c = 0; c < cpus; c++) {
				unsigned s = LINUX64_CPU_SLOT(cpu_mpidr[c]);
				put_str(&q, " "); q = put_dec(q, cpu_hot[s].upcalls); put_str(&q, "/"); q = put_dec(q, cpu_hot[s].last_call);
			}
			*q = 0;
			logs(cb);
		}
		if (perf)
			acct_report();
		if (!xnu_dbg || (lines >= 90 && (++ticks % 10) != 0))
			continue;
		char buf[420]; char *p = buf;
		put_str(&p, "xnu: monitor");
		for (unsigned i = 0; i < sizeof(dec) / sizeof(dec[0]); i++) { put_str(&p, dec[i].name); p = put_dec(p, xnu_dbg[dec[i].i]); }
		put_str(&p, " kfaults="); p = put_dec(p, kfaults);
		put_str(&p, " moved="); p = put_dec(p, kfault_moved);
		for (unsigned i = 0; i < sizeof(hex) / sizeof(hex[0]); i++) { put_str(&p, hex[i].name); put_hex(&p, xnu_dbg[hex[i].i]); }
		*p = 0;
		logs(buf);
		lines++;
	}
	return 0;
}

/* The framebuffer to fb_dump every fb_dump_ms (a host thread: plain Linux context, XNU keeps running on its CPU). */
static void *fb_thread;
static int fb_fn(void *arg)
{
	void *f = fki_file_open(fb_dump, 1, 0);
	(void)arg;
	if (!f) { logs("xnu: cannot open fb_dump (create the file first)"); return 0; }
	while (!fki_thread_should_stop()) {
		fki_msleep(fb_dump_ms);
		fki_file_pwrite(f, (unsigned char *)img.region.va + fb_off, FB_SIZE, 0);
	}
	fki_file_close(f);
	return 0;
}

/*
 * Pure EL0 load-time binary patching:
 *  1. Replace UBSan minimal soft-trap `brk #0xff00` (0xd43fe000) with `nop` (0xd503201f).
 *  2. Replace EL1 ID register reads (`mrs xN, ID_AA64ISAR0_EL1` / `ID_AA64PFR0_EL1`) found in
 *     prelinked kexts with `movz xN, #0` (0xd2800000 | rd) so they take the safe baseline path
 *     without trapping at EL0.
 */
#define S_ATTR_PURE_INSTRUCTIONS 0x80000000U
#define S_ATTR_SOME_INSTRUCTIONS 0x00000400U

static int x_str_bounded_eq(const char *s, u64_t max, const char *lit)
{
	u64_t i = 0;
	while (i < max) {
		if (s[i] != lit[i])
			return 0;
		if (s[i] == 0)
			return 1;
		i++;
	}
	return 0;
}

struct el0_va_span {
	u64_t lo, hi;
};

static int va_in_spans(const struct el0_va_span *spans, unsigned nspans, u64_t va, u64_t len)
{
	if (!len || va + len < va)
		return 0;
	for (unsigned i = 0; i < nspans; i++) {
		if (va >= spans[i].lo && va + len <= spans[i].hi)
			return 1;
	}
	return 0;
}

#define XNU_LC_OK(l, end) \
	((const char *)(end) - (const char *)(l) >= (long)sizeof(struct lc) && \
	 (l)->cmdsize >= sizeof(struct lc) && \
	 (l)->cmdsize <= (u64_t)((const char *)(end) - (const char *)(l)))

static int va_in_loaded_segs_prot(const struct xnu_image *im, u64_t va, u64_t len, u32_t req_prot)
{
	if (!len || va + len < va)
		return 0;
	const struct mh64 *mh = (const void *)im->file;
	const struct lc *l = (const void *)(im->file + sizeof(*mh));
	const struct lc *end = (const void *)(im->file + sizeof(*mh) + mh->sizeofcmds);
	for (u32_t ci = 0; ci < mh->ncmds && XNU_LC_OK(l, end);
	     ci++, l = (const void *)((const char *)l + l->cmdsize)) {
		if (l->cmd != LC_SEGMENT_64 || l->cmdsize < sizeof(struct seg64))
			continue;
		const struct seg64 *s = (const void *)l;
		if (!x_strncmp(s->segname, "__PAGEZERO", 16))
			continue;
		/* Writable segments include zero-filled BSS (vmsize > filesize). */
		u64_t span = (s->initprot & 2) ? s->vmsize : s->filesize;
		if (!span || (s->initprot & req_prot) != req_prot)
			continue;
		u64_t lo;
		if (!macho_va_to_loaded(im, s->vmaddr, span, &lo))
			continue;
		u64_t hi = lo + span;
		if (va >= lo && va + len <= hi)
			return 1;
	}
	return 0;
}

static int va_in_loaded_segs(const struct xnu_image *im, u64_t va, u64_t len)
{
	return va_in_loaded_segs_prot(im, va, len, 1);
}

/* Minimal self-contained SHA-256 (FIPS 180-4) for corecrypto pre-patch verification. */
struct el0_sha256_ctx {
	u32_t h[8];
	u64_t bits;
	unsigned char buf[64];
	unsigned n;
};

static inline u32_t sha256_rotr(u32_t x, unsigned r) { return (x >> r) | (x << (32 - r)); }

static void sha256_compress(u32_t h[8], const unsigned char blk[64])
{
	static const u32_t K[64] = {
		0x428a2f98U, 0x71374491U, 0xb5c0fbcfU, 0xe9b5dba5U, 0x3956c25bU, 0x59f111f1U, 0x923f82a4U, 0xab1c5ed5U,
		0xd807aa98U, 0x12835b01U, 0x243185beU, 0x550c7dc3U, 0x72be5d74U, 0x80deb1feU, 0x9bdc06a7U, 0xc19bf174U,
		0xe49b69c1U, 0xefbe4786U, 0x0fc19dc6U, 0x240ca1ccU, 0x2de92c6fU, 0x4a7484aaU, 0x5cb0a9dcU, 0x76f988daU,
		0x983e5152U, 0xa831c66dU, 0xb00327c8U, 0xbf597fc7U, 0xc6e00bf3U, 0xd5a79147U, 0x06ca6351U, 0x14292967U,
		0x27b70a85U, 0x2e1b2138U, 0x4d2c6dfcU, 0x53380d13U, 0x650a7354U, 0x766a0abbU, 0x81c2c92eU, 0x92722c85U,
		0xa2bfe8a1U, 0xa81a664bU, 0xc24b8b70U, 0xc76c51a3U, 0xd192e819U, 0xd6990624U, 0xf40e3585U, 0x106aa070U,
		0x19a4c116U, 0x1e376c08U, 0x2748774cU, 0x34b0bcb5U, 0x391c0cb3U, 0x4ed8aa4aU, 0x5b9cca4fU, 0x682e6ff3U,
		0x748f82eeU, 0x78a5636fU, 0x84c87814U, 0x8cc70208U, 0x90befffaU, 0xa4506cebU, 0xbef9a3f7U, 0xc67178f2U,
	};
	u32_t w[64], a, b, c, d, e, f, g, hh;
	for (unsigned i = 0; i < 16; i++)
		w[i] = ((u32_t)blk[i * 4] << 24) | ((u32_t)blk[i * 4 + 1] << 16) |
		       ((u32_t)blk[i * 4 + 2] << 8) | (u32_t)blk[i * 4 + 3];
	for (unsigned i = 16; i < 64; i++) {
		u32_t s0 = sha256_rotr(w[i - 15], 7) ^ sha256_rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
		u32_t s1 = sha256_rotr(w[i - 2], 17) ^ sha256_rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
		w[i] = w[i - 16] + s0 + w[i - 7] + s1;
	}
	a = h[0]; b = h[1]; c = h[2]; d = h[3]; e = h[4]; f = h[5]; g = h[6]; hh = h[7];
	for (unsigned i = 0; i < 64; i++) {
		u32_t S1 = sha256_rotr(e, 6) ^ sha256_rotr(e, 11) ^ sha256_rotr(e, 25);
		u32_t ch = (e & f) ^ ((~e) & g);
		u32_t t1 = hh + S1 + ch + K[i] + w[i];
		u32_t S0 = sha256_rotr(a, 2) ^ sha256_rotr(a, 13) ^ sha256_rotr(a, 22);
		u32_t maj = (a & b) ^ (a & c) ^ (b & c);
		u32_t t2 = S0 + maj;
		hh = g; g = f; f = e; e = d + t1;
		d = c; c = b; b = a; a = t1 + t2;
	}
	h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
}

static void sha256_init(struct el0_sha256_ctx *ctx)
{
	ctx->h[0] = 0x6a09e667U; ctx->h[1] = 0xbb67ae85U; ctx->h[2] = 0x3c6ef372U; ctx->h[3] = 0xa54ff53aU;
	ctx->h[4] = 0x510e527fU; ctx->h[5] = 0x9b05688cU; ctx->h[6] = 0x1f83d9abU; ctx->h[7] = 0x5be0cd19U;
	ctx->bits = 0;
	ctx->n = 0;
}

static void sha256_update(struct el0_sha256_ctx *ctx, const unsigned char *p, u64_t len)
{
	ctx->bits += len * 8;
	while (len > 0) {
		unsigned take = (unsigned)(64 - ctx->n);
		if (take > len)
			take = (unsigned)len;
		x_memcpy(ctx->buf + ctx->n, p, take);
		ctx->n += take;
		p += take;
		len -= take;
		if (ctx->n == 64) {
			sha256_compress(ctx->h, ctx->buf);
			ctx->n = 0;
		}
	}
}

static void sha256_final(struct el0_sha256_ctx *ctx, unsigned char out[32])
{
	ctx->buf[ctx->n++] = 0x80;
	if (ctx->n > 56) {
		while (ctx->n < 64)
			ctx->buf[ctx->n++] = 0;
		sha256_compress(ctx->h, ctx->buf);
		ctx->n = 0;
	}
	while (ctx->n < 56)
		ctx->buf[ctx->n++] = 0;
	for (unsigned i = 0; i < 8; i++)
		ctx->buf[56 + i] = (unsigned char)(ctx->bits >> (56 - i * 8));
	sha256_compress(ctx->h, ctx->buf);
	for (unsigned i = 0; i < 8; i++) {
		out[i * 4 + 0] = (unsigned char)(ctx->h[i] >> 24);
		out[i * 4 + 1] = (unsigned char)(ctx->h[i] >> 16);
		out[i * 4 + 2] = (unsigned char)(ctx->h[i] >> 8);
		out[i * 4 + 3] = (unsigned char)(ctx->h[i]);
	}
}

static int parse_sha256_hex(const char *s, unsigned char out[32])
{
	if (!s)
		return 0;
	for (unsigned i = 0; i < 32; i++) {
		unsigned char v = 0;
		for (unsigned k = 0; k < 2; k++) {
			char c = s[i * 2 + k];
			unsigned char d;
			if (c >= '0' && c <= '9') d = (unsigned char)(c - '0');
			else if (c >= 'a' && c <= 'f') d = (unsigned char)(c - 'a' + 10);
			else if (c >= 'A' && c <= 'F') d = (unsigned char)(c - 'A' + 10);
			else return 0;
			v = (unsigned char)((v << 4) | d);
		}
		out[i] = v;
	}
	return s[64] == '\0';
}

static int ct_memeq32(const unsigned char a[32], const unsigned char b[32])
{
	unsigned char diff = 0;
	for (unsigned i = 0; i < 32; i++)
		diff |= (unsigned char)(a[i] ^ b[i]);
	return diff == 0;
}

static void log_sha256(const char *prefix, const unsigned char d[32])
{
	static const char hex[] = "0123456789abcdef";
	char line[160];
	unsigned p = 0;
	while (prefix[p] && p < 80) {
		line[p] = prefix[p];
		p++;
	}
	for (unsigned i = 0; i < 32; i++) {
		line[p++] = hex[d[i] >> 4];
		line[p++] = hex[d[i] & 0xf];
	}
	line[p++] = '\n';
	fki_log(line, p);
}

static void el0_patch_macho_sections(const struct xnu_image *im, const struct mh64 *mh,
				     const struct el0_va_span *cc_sects, unsigned cc_nsects,
				     u64_t *n_ubsan, u64_t *n_idreg,
				     u64_t *n_cc_idreg, u64_t *n_cc_ubsan)
{
	const char *file = (const char *)im->file;
	u64_t file_size = im->file_size;
	u64_t mh_off = (u64_t)((const char *)mh - file);
	if (mh_off > file_size || file_size - mh_off < sizeof(*mh) ||
	    mh->magic != MH_MAGIC_64 ||
	    mh->sizeofcmds > file_size - mh_off - sizeof(*mh) ||
	    mh->ncmds > mh->sizeofcmds / sizeof(struct lc))
		return;
	const struct lc *l = (const void *)(mh + 1);
	const struct lc *end = (const void *)((const char *)l + mh->sizeofcmds);
	for (u32_t ci = 0; ci < mh->ncmds && XNU_LC_OK(l, end);
	     ci++, l = (const void *)((const char *)l + l->cmdsize)) {
		if (l->cmd != LC_SEGMENT_64 || l->cmdsize < sizeof(struct seg64))
			continue;
		const struct seg64 *s = (const void *)l;
		if (!s->vmsize || !(s->initprot & 4))
			continue;
		if ((u64_t)s->nsects > (l->cmdsize - sizeof(*s)) / sizeof(struct sect64))
			continue;
		const struct sect64 *sc = (const void *)(s + 1);
		for (u32_t si = 0; si < s->nsects; si++) {
			if (!(sc[si].flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS)))
				continue;
			if (!sc[si].size || sc[si].addr < s->vmaddr ||
			    sc[si].addr - s->vmaddr > s->vmsize ||
			    sc[si].size > s->vmsize - (sc[si].addr - s->vmaddr))
				continue;
			u64_t sec_va;
			if (!macho_va_to_loaded(im, sc[si].addr, sc[si].size, &sec_va) ||
			    (sec_va & 3) ||
			    !va_in_loaded_segs(im, sec_va, sc[si].size))
				continue;
			u32_t *ins = (u32_t *)sec_va;
			u64_t cnt = sc[si].size / 4;
			for (u64_t i = 0; i < cnt; i++) {
				u32_t w = ins[i];
				if (w == 0xd43fe000U) {			/* brk #0xff00 */
					u64_t va = (u64_t)&ins[i];
					ins[i] = 0xd503201fU;		/* nop */
					__asm__ volatile("dc cvau, %0\n\tdsb ish\n\tic ivau, %0" : : "r"(&ins[i]) : "memory");
					if (va_in_spans(cc_sects, cc_nsects, va, 4))
						(*n_cc_ubsan)++;
					(*n_ubsan)++;
				} else if ((w & 0xffffffe0U) == 0xd5380600U ||	/* mrs xN, ID_AA64ISAR0_EL1 */
					   (w & 0xffffffe0U) == 0xd5380400U) {	/* mrs xN, ID_AA64PFR0_EL1 */
					u32_t rd = w & 0x1fU;
					u64_t va = (u64_t)&ins[i];
					ins[i] = 0xd2800000U | rd;	/* movz xN, #0 */
					__asm__ volatile("dc cvau, %0\n\tdsb ish\n\tic ivau, %0" : : "r"(&ins[i]) : "memory");
					if (va_in_spans(cc_sects, cc_nsects, va, 4))
						(*n_cc_idreg)++;
					if (*n_idreg < 16) {
						logkv("xnu: EL0 patched ID-reg mrs at", va);
						logkv("xnu:   original insn", w);
					}
					(*n_idreg)++;
				}
			}
		}
	}
}

static void el0_patch_image(struct xnu_image *im)
{
	const struct mh64 *mh = (const void *)im->file;
	const struct lc *l = (const void *)(im->file + sizeof(*mh));
	const struct lc *end = (const void *)(im->file + sizeof(*mh) + mh->sizeofcmds);
	u64_t n_ubsan = 0, n_idreg = 0, n_cc_idreg = 0, n_cc_ubsan = 0;
	struct el0_va_span cc_sects[32];
	unsigned cc_nsects = 0;
	int cc_sects_overflow = 0;
	const struct fileset_entry_lc *cc_fe = 0;
	unsigned char cc_digest[32];
	int have_cc_digest = 0;

	/* Locate com.apple.kec.corecrypto executable sections & hash unpatched loaded bytes. */
	if (im->filetype == MH_FILESET) {
		u32_t fi = 0;
		for (const struct lc *fl = l; fi < mh->ncmds && XNU_LC_OK(fl, end);
		     fi++, fl = (const void *)((const char *)fl + fl->cmdsize)) {
			if (fl->cmd != LC_FILESET_ENTRY || fl->cmdsize < sizeof(struct fileset_entry_lc))
				continue;
			const struct fileset_entry_lc *fe = (const void *)fl;
			if (fe->entry_id >= fe->cmdsize ||
			    !x_str_bounded_eq((const char *)fe + fe->entry_id,
					      fe->cmdsize - fe->entry_id,
					      "com.apple.kec.corecrypto"))
				continue;
			if (fe->fileoff > im->file_size || im->file_size - fe->fileoff < sizeof(*mh))
				break;
			const struct mh64 *cmh = (const void *)(im->file + fe->fileoff);
			if (cmh->magic != MH_MAGIC_64 ||
			    cmh->sizeofcmds > im->file_size - fe->fileoff - sizeof(*cmh) ||
			    cmh->ncmds > cmh->sizeofcmds / sizeof(struct lc))
				break;
			cc_fe = fe;
			struct el0_sha256_ctx sctx;
			sha256_init(&sctx);
			int hash_ok = 1;
			const struct lc *cl = (const void *)(cmh + 1);
			const struct lc *cend = (const void *)((const char *)cl + cmh->sizeofcmds);
			for (u32_t ci = 0; ci < cmh->ncmds && XNU_LC_OK(cl, cend);
			     ci++, cl = (const void *)((const char *)cl + cl->cmdsize)) {
				if (cl->cmd != LC_SEGMENT_64 || cl->cmdsize < sizeof(struct seg64))
					continue;
				const struct seg64 *cs = (const void *)cl;
				if (cs->filesize) {
					u64_t seg_va;
					if (!macho_va_to_loaded(im, cs->vmaddr, cs->filesize, &seg_va) ||
					    !va_in_loaded_segs(im, seg_va, cs->filesize)) {
						hash_ok = 0;
					} else {
						sha256_update(&sctx, (const unsigned char *)seg_va, cs->filesize);
					}
				}
				if (!(cs->initprot & 4) || !cs->vmsize)
					continue;
				if ((u64_t)cs->nsects > (cl->cmdsize - sizeof(*cs)) / sizeof(struct sect64))
					continue;
				const struct sect64 *csc = (const void *)(cs + 1);
				for (u32_t si = 0; si < cs->nsects; si++) {
					if (!(csc[si].flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS)))
						continue;
					if (!csc[si].size || csc[si].addr < cs->vmaddr ||
					    csc[si].addr - cs->vmaddr > cs->vmsize ||
					    csc[si].size > cs->vmsize - (csc[si].addr - cs->vmaddr))
						continue;
					u64_t lo;
					if (!macho_va_to_loaded(im, csc[si].addr, csc[si].size, &lo) ||
					    !va_in_loaded_segs(im, lo, csc[si].size))
						continue;
					u64_t hi = lo + csc[si].size;
					if (cc_nsects < sizeof(cc_sects) / sizeof(cc_sects[0])) {
						cc_sects[cc_nsects].lo = lo;
						cc_sects[cc_nsects].hi = hi;
						cc_nsects++;
					} else {
						cc_sects_overflow = 1;
					}
				}
			}
			if (hash_ok) {
				sha256_final(&sctx, cc_digest);
				have_cc_digest = 1;
				log_sha256("xnu: EL0 corecrypto unpatched sha256: ", cc_digest);
			}
			break;
		}
	}

	/* Scan ONLY executable code sections (S_ATTR_PURE/SOME_INSTRUCTIONS), never data. */
	if (im->filetype == MH_FILESET) {
		u32_t fi = 0;
		for (const struct lc *fl = l; fi < mh->ncmds && XNU_LC_OK(fl, end);
		     fi++, fl = (const void *)((const char *)fl + fl->cmdsize)) {
			if (fl->cmd != LC_FILESET_ENTRY || fl->cmdsize < sizeof(struct fileset_entry_lc))
				continue;
			const struct fileset_entry_lc *fe = (const void *)fl;
			if (fe->fileoff > im->file_size || im->file_size - fe->fileoff < sizeof(*mh))
				continue;
			el0_patch_macho_sections(im, (const void *)(im->file + fe->fileoff),
						 cc_sects, cc_nsects,
						 &n_ubsan, &n_idreg, &n_cc_idreg, &n_cc_ubsan);
		}
	} else {
		el0_patch_macho_sections(im, mh, cc_sects, cc_nsects,
					 &n_ubsan, &n_idreg, &n_cc_idreg, &n_cc_ubsan);
	}
	__asm__ volatile("dsb ish\n\tisb" : : : "memory");
	logkv("xnu: EL0 patched brk #0xff00 -> nop count", n_ubsan);
	logkv("xnu: EL0 patched ID-reg mrs -> movz count", n_idreg);
	logkv("xnu: EL0 corecrypto ID-reg patches", n_cc_idreg);

	/*
	 * Preserve _fipspost_post_integrity failure path by default. Only update it when the
	 * operator explicitly provides el0_fips_sha256 matching the pre-patch SHA-256 of
	 * com.apple.kec.corecrypto's loaded segments, exactly 2 approved ID-reg instructions
	 * (and 0 other instructions) were patched in corecrypto, and the symbol lies in a
	 * corecrypto executable section + loaded top-level segment with a valid prologue.
	 */
	unsigned char want_digest[32];
	int fips_digest_ok = (have_cc_digest &&
			      parse_sha256_hex(el0_fips_sha256, want_digest) &&
			      ct_memeq32(cc_digest, want_digest));
	if (fips_digest_ok && cc_fe && !cc_sects_overflow && n_cc_idreg == 2 && n_cc_ubsan == 0) {
		const struct mh64 *cmh = (const void *)(im->file + cc_fe->fileoff);
		const struct lc *cl = (const void *)(cmh + 1);
		const struct lc *cend = (const void *)((const char *)cl + cmh->sizeofcmds);
		for (u32_t ci = 0; ci < cmh->ncmds && XNU_LC_OK(cl, cend);
		     ci++, cl = (const void *)((const char *)cl + cl->cmdsize)) {
			if (cl->cmd != LC_SYMTAB || cl->cmdsize < sizeof(struct symtab_lc))
				continue;
			const struct symtab_lc *st = (const void *)cl;
			if ((u64_t)st->symoff > im->file_size ||
			    (u64_t)st->nsyms > (im->file_size - st->symoff) / sizeof(struct nlist64) ||
			    (u64_t)st->stroff > im->file_size ||
			    (u64_t)st->strsize > im->file_size - st->stroff)
				break;
			const struct nlist64 *nl = (const void *)(im->file + st->symoff);
			const char *strs = (const char *)(im->file + st->stroff);
			for (u32_t j = 0; j < st->nsyms; j++) {
				if ((nl[j].n_type & 0xe0) || !nl[j].n_value || nl[j].n_strx >= st->strsize)
					continue;
				if (x_str_bounded_eq(strs + nl[j].n_strx,
						     st->strsize - nl[j].n_strx,
						     "_fipspost_post_integrity")) {
					u64_t fn_va;
					if (!macho_va_to_loaded(im, nl[j].n_value, 12, &fn_va) ||
					    (fn_va & 3) ||
					    !va_in_spans(cc_sects, cc_nsects, fn_va, 12) ||
					    !va_in_loaded_segs(im, fn_va, 12)) {
						logs("xnu: skipping _fipspost_post_integrity patch (outside corecrypto exec section)");
						break;
					}
					u32_t *fn = (u32_t *)fn_va;
					u32_t p0 = fn[0];
					/* Expect bti c (0xd503245f), pacibsp (0xd503237f), stp, or sub sp. */
					if (p0 == 0xd503245fU || p0 == 0xd503237fU ||
					    (p0 & 0xffc003ffU) == 0xd10003ffU ||
					    (p0 & 0x7e000000U) == 0x28000000U) {
						fn[0] = 0xd503245fU;	/* bti c */
						fn[1] = 0x52800000U;	/* mov w0, #0 */
						fn[2] = 0xd65f03c0U;	/* ret */
						for (int k = 0; k < 3; k++)
							__asm__ volatile("dc cvau, %0\n\tdsb ish\n\tic ivau, %0" : : "r"(&fn[k]) : "memory");
						__asm__ volatile("dsb ish\n\tisb" : : : "memory");
						logkv("xnu: EL0 verified sha256 and patched _fipspost_post_integrity at", fn_va);
					} else {
						logkv("xnu: unexpected _fipspost_post_integrity prologue", p0);
					}
					break;
				}
			}
			break;
		}
	} else if (cc_fe && (n_cc_idreg > 0 || n_cc_ubsan > 0 || cc_sects_overflow)) {
		if (cc_sects_overflow)
			logs("xnu: preserving _fipspost_post_integrity (corecrypto exec section table overflow)");
		else if (!el0_fips_sha256 || !el0_fips_sha256[0])
			logs("xnu: preserving _fipspost_post_integrity (el0_fips_sha256 not set)");
		else if (!fips_digest_ok)
			logs("xnu: preserving _fipspost_post_integrity (el0_fips_sha256 mismatch or invalid hex)");
		else
			logkv("xnu: preserving _fipspost_post_integrity (unexpected corecrypto ubsan patches)", n_cc_ubsan);
	}
}

static int enter_arm_init(struct boot_args_v2 *ba)
{
	if (img.filetype == MH_FILESET) {
		/* kernel collection at its link address: XNU applies the chained fixups itself; nothing slides */
		if (img.slide) { logs("xnu: a kernel collection must not slide"); return -22; }
		logs("xnu: kernel collection: XNU applies its chained fixups (kernel_collection_slide)");
	} else {
		struct fki_pac_keys saved; unsigned long c = fki_enter(&xnu_W.xnu_keys, &saved);
		int n = macho_rebase(&img);
		fki_leave(c, &saved);
		logkv("xnu: threaded fixups applied", (u64_t)n);
		if (n < 0) return n;

		u64_t mh_unslid = 0;
		int segs = macho_slide_header(&img, &mh_unslid);
		logkv("xnu: load commands slid (segments)", (u64_t)segs);
		if (segs < 0) return segs;
	}
	if (el0_mode)
		el0_patch_image(&img);
	u64_t a_init = macho_sym(&img, "_arm_init"), a_hook = macho_sym(&img, "_linux64_hook"),
	      a_slide = macho_sym(&img, "_linux64_slide"), a_el0 = macho_sym(&img, "_linux64_el0"),
	      a_el0_caps = macho_sym(&img, "_linux64_el0_caps"),
	      a_abi = macho_sym(&img, "_linux64_abi_version"),
	      a_host_abi = macho_sym(&img, "_linux64_host_abi_version");
	logkv("xnu: arm_init at", a_init);
	if (!a_init || !a_hook || !a_slide) { logs("xnu: arm_init or linux64_hook missing (kernel without the LINUX64 patch?)"); return -2; }
	if ((a_init & 3) || !va_in_loaded_segs_prot(&img, a_init, 4, 5) ||
	    (a_hook & 7) || !va_in_loaded_segs_prot(&img, a_hook, 8, 3) ||
	    (a_slide & 7) || !va_in_loaded_segs_prot(&img, a_slide, 8, 3)) {
		logs("xnu: invalid boot symbol address or segment permissions");
		return -22;
	}
	if (!a_abi || (a_abi & 3) || !va_in_loaded_segs_prot(&img, a_abi, 4, 1)) {
		logs("xnu: _linux64_abi_version missing or invalid (stale kernel incompatible with host ABI)");
		return -22;
	}
	if (*(unsigned int *)a_abi != LINUX64_ABI_VERSION) {
		logkv("xnu: LINUX64_ABI_VERSION mismatch: kernel has", *(unsigned int *)a_abi);
		logkv("xnu: LINUX64_ABI_VERSION expected by host", LINUX64_ABI_VERSION);
		return -22;
	}
	if (!a_host_abi || (a_host_abi & 3) || !va_in_loaded_segs_prot(&img, a_host_abi, 4, 3)) {
		logs("xnu: _linux64_host_abi_version missing or invalid (stale kernel incompatible with host ABI)");
		return -22;
	}
	*(unsigned int *)a_host_abi = LINUX64_ABI_VERSION;
	if (el0_mode) {
		if (!a_el0 || !a_el0_caps ||
		    (a_el0 & 3) || !va_in_loaded_segs_prot(&img, a_el0, 4, 3) ||
		    (a_el0_caps & 3) || !va_in_loaded_segs_prot(&img, a_el0_caps, 4, 3)) {
			logs("xnu: _linux64_el0 or _linux64_el0_caps missing or invalid (kernel without EL0 patch?)");
			return -2;
		}
		*(unsigned int *)a_el0 = 1;
		unsigned int caps = (fki_caps() & FKI_CAP_EXEC_ONLY) ? LINUX64_EL0_CAP_EXEC_ONLY : 0;
		if (xo_compat)
			caps |= LINUX64_EL0_CAP_XO_COMPAT;
		*(unsigned int *)a_el0_caps = caps;
	}
	*(u64_t *)a_hook = (u64_t)(el0_mode ? xnu_world_upcall_el0 : xnu_world_upcall);
	*(u64_t *)a_slide = img.slide;

	diag_va[0] = a_init; diag_va[1] = img.vmlow; diag_va[2] = (u64_t)ba; diag_va[3] = (u64_t)ba->deviceTreeP;
	diag_va[4] = macho_sym(&img, "_port_crumb");
	if (diag_va[4] && ((diag_va[4] & 7) || !va_in_loaded_segs_prot(&img, diag_va[4], 8, 3))) {
		logs("xnu: ignoring invalid _port_crumb symbol");
		diag_va[4] = 0;
	}
	diag_va[5] = macho_sym(&img, "_strlen");
	if (diag_va[5] && ((diag_va[5] & 3) || !va_in_loaded_segs_prot(&img, diag_va[5], 4, 5))) {
		logs("xnu: ignoring invalid _strlen symbol");
		diag_va[5] = 0;
	}
	{
		u64_t a_dbg = macho_sym(&img, "_linux64_dbg");
		if (a_dbg && ((a_dbg & 7) || !va_in_loaded_segs_prot(&img, a_dbg, 16 * sizeof(unsigned long), 1))) {
			logs("xnu: ignoring invalid _linux64_dbg symbol");
			a_dbg = 0;
		}
		xnu_dbg = (const unsigned long *)a_dbg;

		u64_t a_stat = macho_sym(&img, "_linux64_el0_stat");
		if (a_stat && ((a_stat & 7) ||
		    !va_in_loaded_segs_prot(&img, a_stat,
					    (u64_t)LINUX64_CPU_SLOTS * 2 * LINUX64_EL0_CLASSES * sizeof(unsigned long), 1))) {
			logs("xnu: ignoring invalid _linux64_el0_stat symbol");
			a_stat = 0;
		}
		xnu_el0_stat = (const unsigned long *)a_stat;
	}
	{	/* the helpers run where XNU does not: a CPU running XNU lets Linux in only at XNU's calls into the host */
		int host_cpu = cpus < fki_num_cpus() ? cpus : fki_num_cpus() - 1;
		if (monitor_cpu == -2)
			monitor_cpu = host_cpu;
		if (xnu_dbg && monitor_cpu >= 0)
			monitor = fki_thread_start(monitor_fn, 0, monitor_cpu, "xnu-monitor");
		if (fb_off && fb_dump && fb_dump_ms > 0 && img.region.va)
			fb_thread = fki_thread_start(fb_fn, 0, monitor_cpu >= 0 ? monitor_cpu : host_cpu, "xnu-fb");
	}
	{
		u64_t a_kf = macho_sym(&img, "_linux64_kernel_fault");
		if (a_kf && ((a_kf & 3) || !va_in_loaded_segs_prot(&img, a_kf, 4, 5))) {
			logs("xnu: ignoring invalid _linux64_kernel_fault symbol");
			a_kf = 0;
		}
		xnu_kfault = (void *)a_kf;

		u64_t a_kfr = macho_sym(&img, "_linux64_kernel_fault_recover");
		if (a_kfr && ((a_kfr & 3) || !va_in_loaded_segs_prot(&img, a_kfr, 4, 5))) {
			logs("xnu: ignoring invalid _linux64_kernel_fault_recover symbol");
			a_kfr = 0;
		}
		xnu_kfault_recover = (void *)a_kfr;
	}
	if (!el0_mode && xnu_kfault) {
		/*
		 * XNU's kernel takes page faults all the time (vm_fault on its kernel map). Where the host cannot hand them
		 * over (fki's probe on Linux's fault path did not register on this kernel) the first one is a Linux oops with
		 * XNU's keys and stack live: XNU is not started there.
		 */
		int ferr = fki_fault_register(0xffffffe000000000ULL, 0xfffffffc00000000ULL, xnu_fault);
		if (ferr) {
			logkv("xnu: the host cannot forward XNU's kernel faults: not starting, err", (u64_t)ferr);
			fki_thread_stop(monitor); monitor = 0;
			fki_thread_stop(fb_thread); fb_thread = 0;
			return ferr;
		}
		logs("xnu: kernel faults on XNU addresses go to XNU's vm_fault");
	}
	if (!el0_mode) {
		/*
		 * XNU's user half starts as Linux's TTBR0. Its TCR stays Linux's (T0SZ 16, 4-level): a 4K XNU user pmap's
		 * hardware root (hw_ttep) is an L0 table whose first entry points at its L1 (ttep), as the walk showed.
		 */
		unsigned long long tcr, ttbr0;
		__asm__ volatile("mrs %0, tcr_el1" : "=r"(tcr));
		__asm__ volatile("mrs %0, ttbr0_el1" : "=r"(ttbr0));
		/*
		 * TCR.A1 off in XNU's world: Linux takes the ASID from TTBR1 (A1 = 1), XNU's pmaps put theirs in TTBR0. With
		 * Linux's A1 every XNU user pmap ran under Linux's one ASID, so XNU's per-ASID TLB flushes missed and a freed
		 * pmap's cached table walk (the exec'd child's stack L2) served the parent: a translation fault that repeats
		 * after vm_fault. The world switch drops the local TLB both ways, so the two ASID spaces never mix.
		 */
		xnu_W.xnu_tcr = tcr & ~(1ULL << 22);
		xnu_W.xnu_ttbr0 = ttbr0;
	}
	a_cpu_entry = macho_sym(&img, "_linux64_cpu_entry");
	if (a_cpu_entry && ((a_cpu_entry & 3) || !va_in_loaded_segs_prot(&img, a_cpu_entry, 4, 5))) {
		logs("xnu: ignoring invalid _linux64_cpu_entry symbol: one CPU");
		a_cpu_entry = 0;
		cpus = 1;
	}
	for (int c = 1; c < cpus; c++) {	/* the secondary CPUs' save areas start as the boot CPU's: XNU's keys, user TTBR0/TCR */
		worlds[c].xnu_keys = xnu_W.xnu_keys;
		worlds[c].xnu_tcr = xnu_W.xnu_tcr;
		worlds[c].xnu_ttbr0 = xnu_W.xnu_ttbr0;
	}
	if (cpus > 1 && !a_cpu_entry) { logs("xnu: kernel without linux64_cpu_entry: one CPU"); cpus = 1; }
	for (int c = 1; c < cpus; c++) {	/* each secondary CPU's host thread, waiting for XNU to start that CPU */
		cpu_start_arg[c] = 0;
		cpu_thread[c] = fki_thread_start(cpu_fn, (void *)(unsigned long)c, c, "xnu-cpu");
		if (!cpu_thread[c]) { logkv("xnu: cannot make a thread for CPU, running with CPUs below it:", (u64_t)c); cpus = c; }
	}
	logs("xnu: entering arm_init");
	stop_state = STOP_ACTIVE_ONE;
	in_xnu = 1;
	long r = el0_mode ? xnu_world_call_el0(a_init, (unsigned long)ba) : xnu_world_call(a_init, (unsigned long)ba);
	in_xnu = 0;
	stop_run = 1;		/* the other CPUs end at their next call into the host (kicked out of idle) */
	stop_commit();
	for (int c = 1; c < cpus; c++) {
		for (int t = 0; cpu_running[c] && t < 500; t++) { fki_kick_cpu(c); fki_msleep(10); }
		if (cpu_running[c]) logkv("xnu: CPU did not leave XNU in 5 s:", (u64_t)c);
		fki_thread_stop(cpu_thread[c]);
		cpu_thread[c] = 0;
	}
	if (!el0_mode)
		fki_fault_unregister();
	logkv("xnu: kernel faults forwarded to XNU", kfaults);
	fki_thread_stop(monitor);
	monitor = 0;
	fki_thread_stop(fb_thread);
	fb_thread = 0;
	logkv(r ? "xnu: run ended by stage hook, r =" : "xnu: arm_init returned (unexpected), r =", (u64_t)r);
	return 0;
}

/* M2a: the image in a vmalloc region with synthetic physical addresses. */
static int run_m2a(void)
{
	u64_t image_size = img.vmhigh - img.vmlow;
	struct boot_args_v2 *ba = build_boot(img.region.va, XNU_FAKE_PHYS, img.region.size, 0, image_size);
	if (!ba) return -22;
	return enter_arm_init(ba);
}

/*
 * M2b: XNU at its own link address in the top foreign window, on physically contiguous memory with real
 * physical addresses, so that the page tables XNU builds can be walked by the hardware.
 */
#define XNU_WINDOW_BASE (img.vmlow & ~0x1fffffULL)	/* 2 MiB aligned, at or below the image's vmlow */

static int run_m2b(void)
{
	u64_t image_size = img.vmhigh - img.vmlow;
	if (img.vmlow < 0xffffffe000000000ULL) { logs("xnu: unexpected link address"); return -22; }
	u64_t image_off = img.vmlow - XNU_WINDOW_BASE;
	u64_t want = image_off + image_size + 0x60000 + BOOT_EXTRA + ((u64_t)ram_mb << 20);
	int err = fki_contig_alloc(want, &img.region);
	if (err) { logkv("xnu: contiguous allocation failed, err", (u64_t)err); return err; }
	logkv("xnu: contiguous DRAM at phys", img.region.pa);
	logkv("xnu: contiguous DRAM size", img.region.size);
	if (img.region.pa & 0x1fffff) { logs("xnu: region not 2 MiB aligned"); return -22; }

	err = fki_contig_alloc(4096, &l2page);
	if (err) return err;
	unsigned long long *l2 = l2page.va;
	unsigned first = (XNU_WINDOW_BASE >> 21) & 511;
	/*
	 * One bootstrap L2 table: map what fits in the window's 1 GiB L1 slot. XNU only touches the image, the
	 * boot data and its first bootstrap allocations before it hands us its own kernel L1 entries (stage 210),
	 * which map all of its DRAM, so DRAM beyond the slot needs no bootstrap mapping.
	 */
	u64_t boot_map = img.region.size;
	if (boot_map > ((u64_t)(512 - first) << 21)) boot_map = (u64_t)(512 - first) << 21;
	if (boot_map < image_off + image_size + 0x60000 + BOOT_EXTRA + (64ULL << 20)) {
		logs("xnu: image and boot data do not fit in the bootstrap window"); return -22;
	}
	logkv("xnu: bootstrap window base", XNU_WINDOW_BASE);
	logkv("xnu: bootstrap window bytes", boot_map);
	for (u64_t o = 0; o < boot_map; o += 0x200000) {
		/* 2 MiB block: normal WB (AttrIdx 0 in Linux's MAIR), inner shareable, AF, EL1 RWX, UXN */
		l2[first + (o >> 21)] = (img.region.pa + o) | (1ULL << 54) | (1ULL << 10) | (3ULL << 8) | 1;
	}
	unsigned long long *l1; fki_u64 l1pa;
	err = fki_window_l1(XNU_WINDOW_BASE, &l1, &l1pa);
	if (err) { logs("xnu: no L1 table for the top window"); return err; }
	unsigned idx = (XNU_WINDOW_BASE >> 30) & 511;
	logkv("xnu: top window L1 table at phys", l1pa);
	logkv("xnu: L1 slot", idx);
	if (l1[idx]) { logkv("xnu: L1 slot already used by Linux:", l1[idx]); return -16; }
	window_slot = &l1[idx];
	window_l1 = l1;
	window_owned[idx] = 1;
	fki_window_set(window_slot, l2page.pa | 3);
	logs("xnu: bootstrap window mapped");

	unsigned char *base = (unsigned char *)XNU_WINDOW_BASE;
	err = macho_copy(&img, base + image_off);
	if (err) { logkv("xnu: macho_copy failed, err", (u64_t)err); return err; }
	img.slide = 0;
	struct boot_args_v2 *ba = build_boot(base, img.region.pa, img.region.size, image_off, image_size);
	if (!ba) return -22;
	return enter_arm_init(ba);
}

/* Pure EL0 host M2b: host manages the high window via fki_kmem_alloc; no contiguous multi-GB RAM needed. */
static int run_m2b_el0(void)
{
	u64_t image_size = img.vmhigh - img.vmlow;
	if (img.vmlow < 0xffffffe000000000ULL) { logs("xnu: unexpected link address"); return -22; }
	if (ram_mb <= 0 || ram_mb > 65536) { logs("xnu: invalid ram_mb"); return -22; }
	u64_t image_off = img.vmlow - XNU_WINDOW_BASE;
	u64_t ram_bytes = (u64_t)ram_mb << 20;
	u64_t overhead = image_off + image_size + 0x60000 + BOOT_EXTRA;
	if (overhead < image_off || ram_bytes > (~0ULL) - overhead) { logs("xnu: ram_mb overflow"); return -22; }
	u64_t want = overhead + ram_bytes;
	/*
	 * All of DRAM goes below the kernel (LINUX64_EL0_APERTURE_VA: XNU's physical aperture, the host's permanent
	 * linear mapping in 2 MiB frames), so that it takes none of the room after the kernel's link address, which
	 * the kernel map's ranges need whole: at the kernel's base it left them too little above about 8 GiB. The
	 * image and the boot data are mapped a second time at the kernel's base, where they are linked to run
	 * (fki_kmem_alias: the same frames, 2 MiB ones where the aperture has them, and as permanent).
	 */
	if (XNU_WINDOW_BASE < LINUX64_EL0_APERTURE_END || want > LINUX64_EL0_APERTURE_END - LINUX64_EL0_APERTURE_VA - 0x200000) {
		logs("xnu: ram_mb does not fit below the kernel (or an unexpected link address)"); return -22;
	}
	int err = fki_kmem_alloc((unsigned long)want, (unsigned long)LINUX64_EL0_APERTURE_VA, XNU_FAKE_PHYS, &img.region);
	if (err) { logkv("xnu: EL0 kmem allocation failed, err", (u64_t)err); return err; }
	logkv("xnu: EL0 aperture base", (u64_t)img.region.va);
	logkv("xnu: EL0 aperture size", img.region.size);
	logkv("xnu: EL0 synthetic phys base", img.region.pa);

	unsigned char *base = (unsigned char *)XNU_WINDOW_BASE;
	u64_t head = (overhead + 0x1fffff) & ~0x1fffffULL;	/* at least what build_boot fills; cut to topOfKernelData below */
	err = fki_kmem_alias(&img.region, (unsigned long)XNU_WINDOW_BASE, (unsigned long)head);
	if (err) { logkv("xnu: EL0 mapping at the kernel's base failed, err", (u64_t)err); return err; }
	err = macho_copy(&img, base + image_off);
	if (err) { logkv("xnu: macho_copy failed, err", (u64_t)err); return err; }
	img.slide = 0;
	struct boot_args_v2 *ba = build_boot(base, img.region.pa, img.region.size, image_off, image_size);
	if (!ba) return -22;
	/* the kernel's static range ends at topOfKernelData rounded up to 2 MiB (arm_vm_init); its heap starts there */
	u64_t top = (ba->topOfKernelData - img.region.pa + 0x1fffff) & ~0x1fffffULL;
	if (top > head) { logs("xnu: boot data beyond the mapping at the kernel's base"); return -22; }
	if (top < head) {	/* what follows is the kernel map's */
		err = fki_kmem_alias(&img.region, (unsigned long)XNU_WINDOW_BASE, (unsigned long)top);
		if (err) { logkv("xnu: EL0 mapping at the kernel's base could not be cut, err", (u64_t)err); return err; }
	}
	logkv("xnu: EL0 kernel static range base", XNU_WINDOW_BASE);
	logkv("xnu: EL0 kernel static range end", XNU_WINDOW_BASE + top);
	return enter_arm_init(ba);
}

static void xnu_cleanup(void)
{
	for (int i = 0; i < LINUX64_CPU_SLOTS; i++) {
		if (cpu_hot[i].utcb) {
			fki_utcb_destroy(cpu_hot[i].utcb);
			cpu_hot[i].utcb = 0;
			cpu_hot[i].utcb_thread = 0;
			cpu_hot[i].utcb_fpu_live = 0;
			cpu_hot[i].utcb_disable_jop = -1;
		}
	}
	if (window_l1) {
		int last = -1;
		for (int i = 0; i < 512; i++) if (window_owned[i]) { window_l1[i] = 0; window_owned[i] = 0; last = i; }
		if (last >= 0) fki_window_set(&window_l1[last], 0);
		window_l1 = 0;
		window_slot = 0;
	}
	if (mode == 3) {
		if (el0_mode) {
			if (img.region.va) fki_kmem_free(&img.region);
		} else {
			if (img.region.va) fki_contig_free(&img.region);
			if (l2page.va) fki_contig_free(&l2page);
		}
	} else if (img.region.va) {
		fki_region_free(&img.region);
	}
	img.region.va = 0;
	img.region.handle = 0;
	l2page.va = 0;
	l2page.handle = 0;
	if (blob) { fki_blob_put(blob); blob = 0; }
	if (rd_blob) { fki_blob_put(rd_blob); rd_blob = 0; }
	if (tc_blob) { fki_blob_put(tc_blob); tc_blob = 0; }
	if (hd_file) { fki_file_close(hd_file); hd_file = 0; }
	if (ks_file) { fki_file_close(ks_file); ks_file = 0; }
	if (dk_file) { fki_file_close(dk_file); dk_file = 0; }
	if (ci_file) { fki_file_close(ci_file); ci_file = 0; }
	if (in_file) { fki_file_close(in_file); in_file = 0; }
}

static int __init xnu_init(void)
{
	const void *data; unsigned long size;
	u64_t extra;
	int err;

	if (fki_version() != FKI_VERSION)
		return -22;
	/* (up to 65536 was taken, and the hosted kernel panicked on the memory once the host had found it) */
	if (ram_mb <= 0 || (u64_t)ram_mb << 20 >= XNU_MEM_MAX) {
		logkv("xnu: ram_mb must be 1 or more, and less than (MiB)", XNU_MEM_MAX >> 20);
		return -22;
	}
	/* (one that is not: root_uuid was left out without a word, the two others went to XNU, which does not take them) */
	if (!uuid_ok(root_uuid) || !uuid_ok(boot_uuid) || !uuid_ok(preboot_uuid)) {
		logs("xnu: root_uuid, boot_uuid and preboot_uuid are UUIDs: 8-4-4-4-12 hexadecimal digits");
		return -22;
	}
	el0_mode = !!(fki_caps() & FKI_CAP_EL0);
	logkv("xnu: FKI caps", fki_caps());
	if (el0_mode && !(fki_caps() & FKI_CAP_PAC)) {
		logs("xnu: EL0 mode requires FKI_CAP_PAC");
		return -95;
	}
	/* every random number XNU takes comes from Linux's generator (the boot seeds, LINUX64_CALL_RANDOM): seeded first */
	err = fki_random_wait();
	if (err) { logkv("xnu: Linux's random number generator is not seeded, err", (u64_t)err); return err; }
	if (ramdisk) {
		err = fki_blob_get(ramdisk, &rd_data, &rd_size, &rd_blob);
		if (err) { logkv("xnu: cannot read the RAM disk, err", (u64_t)err); goto fail; }
	}
	if (trustcache) {
		err = fki_blob_get(trustcache, &tc_data, &tc_size, &tc_blob);
		if (err) { logkv("xnu: cannot read the trust cache, err", (u64_t)err); goto fail; }
	}
	if (console_in) {
		fki_u64 sz = 0;
		ci_file = fki_file_open(console_in, 0, &sz);
		ci_off = sz;	/* only what is written after the load */
		if (!ci_file) logs("xnu: cannot open console_in");
	}
	if (keystore) {
		fki_u64 sz;
		ks_file = fki_file_open(keystore, 1, &sz);
		if (!ks_file) logs("xnu: cannot open the keystore file");
	}
	if (input_in) {
		fki_u64 sz = 0;
		in_file = fki_file_open(input_in, 0, &sz);
		in_off = sz & ~7ULL;	/* only what is written after the load */
		if (!in_file) logs("xnu: cannot open input_in");
	}
	if (disk) {
		dk_file = fki_file_open(disk, 1, &dk_size);
		if (!dk_file) { logs("xnu: cannot open the block disk"); err = -2; goto fail; }
		logkv("xnu: block disk bytes", dk_size);
	}
	if (hostdisk) {
		hd_file = fki_file_open(hostdisk, hostdisk_rw, &hd_size);
		if (!hd_file) { logs("xnu: cannot open the host disk"); err = -2; goto fail; }
		logkv("xnu: host disk bytes", hd_size);
	}
	err = fki_blob_get(image, &data, &size, &blob);
	if (err) { logkv("xnu: cannot read image, err", (u64_t)err); goto fail; }
	/*
	 * run_m2a's region (every mode but 1 and 3): the image, room for the boot data build_boot puts after it (run_m2b's
	 * sum), then ram_mb. The RAM alone left a RAM disk or a framebuffer larger than it to be written past the region.
	 */
	extra = 0x60000 + BOOT_EXTRA + ((u64_t)ram_mb << 20);
	if (extra < BOOT_EXTRA) { logs("xnu: ram_mb overflow"); err = -22; goto fail; }
	err = macho_load(&img, data, size, mode == 3 ? ~0UL : mode == 1 ? 0 : (unsigned long)extra);
	if (err) { logkv("xnu: load failed", (u64_t)err); goto fail; }
	logkv("xnu: image vmlow", img.vmlow);
	logkv("xnu: image size", img.vmhigh - img.vmlow);
	if (mode != 3) {
		logkv("xnu: region at", (u64_t)img.region.va);
		logkv("xnu: region size", img.region.size);
	}

	fki_random(&xnu_W.xnu_keys, sizeof(xnu_W.xnu_keys));
	xnu_W.xnu_keys.sctlr_en = ~0ULL;
	fki_random(&xnu_user_keys, sizeof(xnu_user_keys));
	xnu_user_keys.sctlr_en = ~0ULL;

	err = mode == 1 ? run_m1() : mode == 3 ? (el0_mode ? run_m2b_el0() : run_m2b()) : run_m2a();
	if (err)
		goto fail;
	return 0;

fail:
	xnu_cleanup();
	return err;
}

static void __exit xnu_exit(void)
{
	xnu_cleanup();
}

module_init(xnu_init);
module_exit(xnu_exit);
MODULE_LICENSE("Proprietary");	/* APSL-2.0 side: not GPL-compatible; taints the kernel like ZFS */
MODULE_DESCRIPTION("XNU personality (PoC)");

// SPDX-License-Identifier: APSL-2.0
/*
 * xnu.ko's loader in user space: xnu_main.c, macho.c and adt.c as they are, loading images this test makes, as insmod
 * with the same parameters would. run.sh builds it with AddressSanitizer and runs it.
 *
 * In place of the rest of the kernel: linux/module.h (next to this file) for <linux/module.h>; below, the fki_*
 * functions (fki_region_alloc is calloc of the page-rounded size, so a write past the region is reported where the
 * kernel has the guard page after a vmalloc area; a firmware file is a copy of exactly the image's size) and world.S
 * (XNU is not run: entering it comes back as if its first call into the host were a halt, and what it was entered
 * with is kept). The host is a case's to shape: its CPUs and their MPIDRs, its counter, what it can do (host_*).
 * The three files' inline AArch64 statements (system registers, PAC, cache maintenance, the stop state's
 * compare-and-swap) are compiled out and what they would have set reads as 0, so the test runs on any host; nothing it
 * checks depends on them.
 *
 * A case is one insmod, in a process of its own: the module's state is static, and a bad access ends the process.
 *
 * What the module hands XNU (the boot arguments, the device tree, the memory after the image) is read back with
 * reader.c, which is written from XNU's readers and includes nothing of the module: boot_data() below.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include "reader.h"

#define __asm__
#define asm
#define volatile(...)		/* of `__asm__ volatile(...)`: the qualifier is never followed by a parenthesis */
#define target(...) unused	/* __attribute__((target("branch-protection=none"))) is AArch64's */
#include "../xnu_main.c"
#include "../macho.c"
#include "../adt.c"

/* ---- the host (fki.h) ---- */
static struct { const char *name; const void *data; unsigned long size; } firmware[3];
static int held;			/* regions and firmware files the module has and has not given back */
static char last_log[200], cc_sha[65];
/* the host a case runs on: Linux at EL1, 4 CPUs whose MPIDRs are their numbers, a 24 MHz counter, 1 GB to give */
static unsigned long host_caps = FKI_CAP_EXEC_ONLY | FKI_CAP_PAC;	/* fki_arm64.c's */
static int host_cpus = 4, host_files;	/* host_files: its files can be opened (hostdisk=) */
static fki_u64 host_hz = 24000000, (*host_mpidr)(int cpu);
static unsigned long host_head;		/* not 0: a region of more than 1 GB is given, with only this much of it behind it */
static struct { unsigned char *va; unsigned long size; } region;	/* the region the module was given */
static struct { unsigned long fn, arg; int calls; } entered;	/* what XNU was entered with */
static fki_u64 asked;			/* what mode 3 last asked for as XNU's memory (a test has none to give) */

void test_bug(void) { printf("BUG()\n"); abort(); }
int fki_version(void) { return FKI_VERSION; }
unsigned long fki_caps(void) { return host_caps; }
void fki_log(const char *s, unsigned long len)
{
	static const char sha[] = "xnu: EL0 corecrypto unpatched sha256: ";

	printf("fki: %.*s\n", (int)len, s);
	snprintf(last_log, sizeof(last_log), "%.*s", (int)len, s);
	if (len >= sizeof(sha) - 1 + 64 && !memcmp(s, sha, sizeof(sha) - 1))
		snprintf(cc_sha, sizeof(cc_sha), "%.64s", s + sizeof(sha) - 1);
}
void fki_log_alert(const char *s, unsigned long len) { fki_log(s, len); }
int fki_region_alloc(unsigned long size, struct fki_region *r)
{
	unsigned long n = (size + 4095) & ~4095UL, behind = n;	/* fki_arm64.c: vzalloc of PAGE_ALIGN(size) */

	/* 1 GB is all a test's host has; of more, the head alone (the module writes the image and the boot data: not XNU's RAM) */
	if (n > 1UL << 30) behind = host_head;
	if (n < size || !behind || !(r->va = calloc(1, behind))) return -12;
	r->pa = 0x100000000ULL; r->size = n; r->handle = 0; held++;
	region.va = r->va; region.size = n;
	return 0;
}
void fki_region_free(struct fki_region *r) { free(r->va); held--; }
int fki_blob_get(const char *name, const void **data, unsigned long *size, void **handle)
{
	for (unsigned i = 0; i < sizeof(firmware) / sizeof(firmware[0]); i++)
		if (firmware[i].name && !strcmp(firmware[i].name, name)) {
			*data = *handle = memcpy(malloc(firmware[i].size), firmware[i].data, firmware[i].size);
			*size = firmware[i].size; held++;
			return 0;
		}
	return -2;
}
void fki_blob_put(void *handle) { free(handle); held--; }
unsigned long fki_enter(const struct fki_pac_keys *foreign, struct fki_pac_keys *saved) { return 0; }
void fki_leave(unsigned long cookie, const struct fki_pac_keys *saved) {}
int fki_pac_swap(const struct fki_pac_keys *load, struct fki_pac_keys *save) { return 0; }
fki_u64 fki_counter_frequency(void) { return host_hz; }
fki_u64 fki_counter(void) { static fki_u64 c; return c += 1000; }
void fki_random(void *buf, unsigned long len) { memset(buf, 0x5a, len); }
int fki_random_wait(void) { return 0; }
void fki_realtime(fki_u64 *sec, fki_u64 *nsec) { *sec = *nsec = 0; }
int fki_bind_cpu(int cpu) { return 0; }
int fki_num_cpus(void) { return host_cpus; }
int fki_cpu_mpidr(int cpu, fki_u64 *mpidr) { *mpidr = host_mpidr ? host_mpidr(cpu) : (fki_u64)cpu; return 0; }
void fki_kick_cpu(int cpu) {}
void fki_idle_until(fki_u64 deadline, fki_u64 max_ns, const volatile unsigned int *wake) {}
void fki_yield(void) {}
void fki_msleep(unsigned int ms) {}
void fki_icache_flush(unsigned long va, unsigned long size) {}
int fki_fault_register(unsigned long lo, unsigned long hi, int (*h)(unsigned long, unsigned long, void *)) { return 0; }
void fki_fault_unregister(void) {}
unsigned long fki_regs_pc(void *regs) { return 0; }
unsigned long fki_regs_lr(void *regs) { return 0; }
unsigned long fki_regs_pstate(void *regs) { return 0; }
unsigned long fki_regs_reg(void *regs, unsigned int n) { return 0; }
void fki_regs_set_pc(void *regs, unsigned long pc) {}
void fki_regs_set_reg(void *regs, unsigned int n, unsigned long v) {}
/* no helper thread runs, no network or host calls in a test; the rest as fki_arm64.c answers for an EL0 host's calls */
void *fki_thread_start(int (*fn)(void *), void *arg, int cpu, const char *name) { return &held; }	/* made, and never run */
void fki_thread_stop(void *thread) {}
int fki_thread_should_stop(void) { return 1; }
void fki_thread_wait(const volatile unsigned long *flag) {}
void fki_thread_wake(void *thread) {}
void *fki_file_open(const char *path, int writable, fki_u64 *size) { if (size) *size = 1 << 20; return host_files ? &host_files : 0; }
long fki_file_rw(void *file, int write, void *buf, unsigned long len, fki_u64 off) { return -9; }
long fki_file_pwrite(void *file, const void *buf, unsigned long len, fki_u64 off) { return -9; }
void fki_file_close(void *file) {}
long fki_net_send(const void *frame, unsigned long len) { return -19; }
long fki_net_recv(void *buf, unsigned long len) { return 0; }
long fki_hostcall(const void *req, unsigned long len, void *reply, unsigned long max) { return -19; }
int fki_contig_alloc(unsigned long size, struct fki_region *r) { asked = size; return -12; }
void fki_contig_free(struct fki_region *r) {}
int fki_window_l1(unsigned long va, unsigned long long **l1_va, fki_u64 *l1_pa) { return -95; }
void fki_window_set(unsigned long long *slot, fki_u64 desc) {}
int fki_kmem_alloc(unsigned long size, unsigned long high_va, fki_u64 pa_base, struct fki_region *r) { asked = size; return -12; }
int fki_kmem_alias(struct fki_region *r, unsigned long va, unsigned long size) { return -95; }
void fki_kmem_free(struct fki_region *r) {}
int fki_high_map(unsigned long va, fki_u64 xpa, unsigned long size, int prot) { return -95; }
void fki_high_unmap(unsigned long va, unsigned long size) {}
void fki_high_flush(void) {}
int fki_high_txn_begin(unsigned long va, unsigned long size, void **handle_out) { *handle_out = 0; return -95; }
void fki_high_txn_restore(void *txn) {}
void fki_high_txn_end(void *txn) {}
int fki_high_read_u64x2(unsigned long va, fki_u64 *out0, fki_u64 *out1) { return -14; }
void *fki_as_create(void) { return 0; }
void fki_as_destroy(void *as) {}
int fki_as_map(void *as, unsigned long va, fki_u64 xpa, unsigned long size, int prot) { return -95; }
void fki_as_unmap(void *as, unsigned long va, unsigned long size) {}
void fki_as_flush(void *as) {}
void *fki_utcb_create(void *as) { return 0; }
void fki_utcb_destroy(void *utcb) {}
int fki_utcb_set_as(void *utcb, void *as) { return -95; }
int fki_utcb_set_pac(void *utcb, const struct fki_pac_keys *keys) { return -95; }
int fki_utcb_run(void *utcb, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop) { return -95; }
int fki_utcb_resume(void *utcb, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop) { return -38; }
int fki_utcb_sync_regs(void *utcb, struct fki_utcb_regs *regs) { return -38; }
int fki_utcb_fpu_read(void *utcb, void *buf, unsigned long len) { return -95; }
int fki_utcb_fpu_write(void *utcb, const void *buf, unsigned long len) { return -95; }

/* ---- world.S ---- */
long xnu_world_call(unsigned long fn, unsigned long arg)
{
	entered.fn = fn; entered.arg = arg; entered.calls++;
	stop_state |= STOP_COMMIT;	/* a call that ends the run commits the stop: by the compare-and-swap, here */
	return xnu_hook(LINUX64_CALL_HALT, 0, 0);
}
long xnu_world_call_el0(unsigned long fn, unsigned long arg) { return xnu_world_call(fn, arg); }
void xnu_world_upcall(void) {}
void xnu_world_upcall_el0(void) {}
void xnu_world_fault_abort(void) {}
void xnu_fault_tramp(void) {}

/* ---- images: file offset = address - KV ---- */
#define KV 0xfffffff007004000ULL	/* where a kernel is linked */
#define PG 0x4000UL
static unsigned char file[8 * PG];	/* the image being made */
static struct mh64 *mh;			/* its Mach-O header being filled, and where that header's next load command goes */
static unsigned char *lc;
static struct symtab_lc *st;

static void header(unsigned long off, u32_t filetype)
{
	mh = (void *)(file + off); lc = (void *)(mh + 1);
	mh->magic = MH_MAGIC_64; mh->cputype = 0x0100000c; mh->filetype = filetype;
}
static void *cmd(u32_t c, u32_t size)
{
	struct lc *l = (void *)lc;

	l->cmd = c; l->cmdsize = size; lc += size; mh->ncmds++; mh->sizeofcmds += size;
	return l;
}
static struct seg64 *seg(const char *name, unsigned long off, unsigned long size, u32_t prot)
{
	struct seg64 *s = cmd(LC_SEGMENT_64, sizeof(*s));

	strncpy(s->segname, name, 16); s->vmaddr = KV + off; s->vmsize = s->filesize = size; s->fileoff = off;
	s->maxprot = s->initprot = prot;
	return s;
}
static void sect(struct seg64 *s, const char *name, u64_t addr, u64_t size, u32_t flags)	/* right after its segment */
{
	struct sect64 *c = (void *)lc;

	strncpy(c->sectname, name, 16); memcpy(c->segname, s->segname, 16); c->addr = addr; c->size = size; c->flags = flags;
	lc += sizeof(*c); s->cmdsize += sizeof(*c); s->nsects++; mh->sizeofcmds += sizeof(*c);
}
static void entry(const char *id, unsigned long off)
{
	struct fileset_entry_lc *e = cmd(LC_FILESET_ENTRY, sizeof(*e) + 32);

	e->vmaddr = KV + off; e->fileoff = off; e->entry_id = sizeof(*e); strcpy((char *)(e + 1), id);
}
static void symtab(unsigned long off)	/* the symbols at off, their names 1 KB later */
{
	st = cmd(LC_SYMTAB, sizeof(*st)); st->symoff = off; st->stroff = off + 0x400; st->strsize = 1;
}
static void sym(const char *name, u64_t value)
{
	struct nlist64 *n = (struct nlist64 *)(file + st->symoff) + st->nsyms++;

	n->n_strx = st->strsize; n->n_type = 0x0f; n->n_sect = 1; n->n_value = value;	/* N_SECT | N_EXT */
	strcpy((char *)file + st->stroff + st->strsize, name); st->strsize += strlen(name) + 1;
}

/*
 * A bare kernel as xnu.ko boots one in mode 2: __TEXT (the header, __thread_starts at ts_addr, code), __DATA (the
 * linux64_* variables, and three pointers to rebase: a chain of two, and one in the image's last 8 bytes) and its symbols.
 * fixup: the first threaded fixup's offset. Installed as the default image.
 */
#define FIXUP (2 * PG + 0x100)
#define TARGET (KV + PG)
static void kernel(u64_t ts_addr, u32_t fixup)
{
	const u64_t ptr = 0xffULL << 43 | (TARGET & 0x7ffffffffffULL);	/* a rebase of TARGET, the chain's last */
	const u32_t ts[4] = { 0, fixup, 3 * PG - 8, 0xffffffff };	/* 4-byte stride; the fixups; the end */
	struct seg64 *text;

	header(0, 2);	/* MH_EXECUTE */
	text = seg("__TEXT", 0, 2 * PG, 5);
	sect(text, "__text", KV + PG, PG, S_ATTR_PURE_INSTRUCTIONS);
	sect(text, "__thread_starts", ts_addr, sizeof(ts), 0);
	seg("__DATA", 2 * PG, PG, 3);
	symtab(3 * PG);
	sym("_arm_init", TARGET);
	sym("_linux64_abi_version", KV + PG + 0x100);
	sym("_linux64_hook", KV + 2 * PG);
	sym("_linux64_slide", KV + 2 * PG + 8);
	sym("_linux64_host_abi_version", KV + 2 * PG + 16);
	sym("_linux64_el0", KV + 2 * PG + 24);
	sym("_linux64_el0_caps", KV + 2 * PG + 28);
	memcpy(file + 0x1000, ts, sizeof(ts));
	*(u32_t *)(file + PG + 0x100) = LINUX64_ABI_VERSION;
	*(u64_t *)(file + FIXUP) = ptr | 2ULL << 51;	/* the next one two strides on */
	*(u64_t *)(file + FIXUP + 8) = *(u64_t *)(file + 3 * PG - 8) = ptr;
	firmware[0].name = image; firmware[0].data = file; firmware[0].size = st->stroff + st->strsize;
}

/*
 * A kernel collection: the kernel's entry (only its symbol table matters here) and com.apple.kec.corecrypto's, whose
 * text reads an ID register three times. Returns the file's size.
 */
static unsigned long collection(void)
{
	header(0, MH_FILESET);
	seg("__TEXT", 0, 3 * PG, 5);
	seg("__TEXT_EXEC", 3 * PG, PG, 5);
	entry("com.apple.kernel", PG);
	entry("com.apple.kec.corecrypto", 2 * PG);
	header(2 * PG, 11);	/* MH_KEXT_BUNDLE */
	sect(seg("__TEXT_EXEC", 3 * PG, PG, 5), "__text", KV + 3 * PG, PG, S_ATTR_PURE_INSTRUCTIONS);
	for (int i = 0; i < 3; i++)
		*(u32_t *)(file + 3 * PG + 16 + 4 * i) = 0xd5380600 + i;	/* mrs x<i>, ID_AA64ISAR0_EL1 */
	header(PG, 2);
	symtab(4 * PG);
	sym("_arm_init", KV + 3 * PG);
	return st->stroff + st->strsize;
}

/* ---- what XNU is handed, read back with reader.c ---- */
static int bad;
#define CK(ok, ...) do { if (!(ok)) { printf("test: "); printf(__VA_ARGS__); printf("\n"); bad = 1; } } while (0)
#define X(v) ((unsigned long long)(v))

/* what a case asked for, in XNU's terms; as it stands, the module's defaults on the default host */
static struct {
	int cpus; unsigned long ecpus;
	const char *model, *soc, *gpu, *root_uuid, *boot_uuid, *preboot_uuid, *args;
	int fb_w, fb_h, vm16k, md0;	/* md0: the root is a RAM disk or a host disk (rd=md0) */
} want = { 1, 0, "VirtualMac2,1", 0, 0, 0, 0, 0, 0, 1280, 800, 1, 0 };

static const struct xr_node *dt_root;
static const struct xr_node *at(const char *path)
{
	const struct xr_node *n = xr_lookup(dt_root, path);

	CK(n, "the device tree has no %s", path);
	return n;
}
/* a property that is there once, len bytes long (0: of any length) */
static const struct xr_prop *prop(const struct xr_node *n, const char *path, const char *name, unsigned len)
{
	const struct xr_prop *p = xr_prop(n, name);

	if (!n) return 0;
	CK(xr_prop_count(n, name) == 1, "%s has %u properties \"%s\", not one", path, xr_prop_count(n, name), name);
	if (p && len) CK(p->len == len, "%s \"%s\" is %u bytes, its reader takes %u", path, name, p->len, len);
	return p && (!len || p->len == len) ? p : 0;
}
static void str(const struct xr_node *n, const char *path, const char *name, const char *is)
{
	const struct xr_prop *p = prop(n, path, name, 0);

	if (p) CK(xr_str_eq(p, is), "%s \"%s\" is not the string \"%s\" with its terminator (%u bytes: \"%.*s\")", path, name, is, p->len,
	    (int)p->len, p->val);
}
static void u32(const struct xr_node *n, const char *path, const char *name, uint32_t is)
{
	const struct xr_prop *p = prop(n, path, name, 4);

	if (p) CK(xr_u32(p, 0) == is, "%s \"%s\" is %#x, not %#x", path, name, xr_u32(p, 0), is);
}
static void u64(const struct xr_node *n, const char *path, const char *name, uint64_t is)
{
	const struct xr_prop *p = prop(n, path, name, 8);

	if (p) CK(xr_u64(p, 0) == is, "%s \"%s\" is %#llx, not %#llx", path, name, X(xr_u64(p, 0)), X(is));
}
static void none(const struct xr_node *n, const char *path, const char *name)
{
	if (n) CK(!xr_prop(n, name), "%s has a \"%s\" nobody asked for", path, name);
}
/* XNU finds a property by its name and a child by its "name": every node has one, and no two of either are alike */
static void nodes(const struct xr_node *n, const char *path)
{
	char sub[300];

	CK(xr_name(n), "%s: no \"name\" that is a string", path);
	for (unsigned i = 0; i < n->nprops; i++)
		for (unsigned j = 0; j < i; j++)
			CK(strcmp(n->props[i].name, n->props[j].name), "%s: two properties \"%s\"", path, n->props[i].name);
	for (const struct xr_node *c = n->child; c; c = c->next) {
		for (const struct xr_node *d = n->child; d != c; d = d->next)
			CK(!xr_name(c) || !xr_name(d) || strcmp(xr_name(c), xr_name(d)), "%s: two children \"%s\"", path, xr_name(c));
		snprintf(sub, sizeof(sub), "%s/%s", n->parent ? path : "", xr_name(c) ? xr_name(c) : "?");
		nodes(c, sub);
	}
}
/* a UUID as uuid_parse takes one (kern.bootuuid, kern.apfsprebootuuid) */
static int is_uuid(const char *s)
{
	if (strlen(s) != 36) return 0;
	for (int i = 0; i < 36; i++)
		if (i == 8 || i == 13 || i == 18 || i == 23 ? s[i] != '-' : !strchr("0123456789abcdefABCDEF", s[i])) return 0;
	return 1;
}

/* what lies after the image in XNU's memory: each inside it, below topOfKernelData (XNU's own pages start there), none over another */
static struct { const char *what; uint64_t pa, len; } placed[8];
static unsigned nplaced;
static int place(const char *what, uint64_t pa, uint64_t len, uint64_t lo, uint64_t hi)
{
	int ok = pa >= lo && pa <= hi && len <= hi - pa;

	CK(ok, "%s (%#llx, %#llx bytes) is not between the image's end and topOfKernelData (%#llx to %#llx)", what, X(pa), X(len), X(lo), X(hi));
	for (unsigned i = 0; i < nplaced; i++)
		CK(pa + len <= placed[i].pa || placed[i].pa + placed[i].len <= pa, "%s lies over %s", what, placed[i].what);
	placed[nplaced].what = what; placed[nplaced].pa = pa; placed[nplaced++].len = len;
	return ok;
}

/* a boot-arg on the command line, as XNU finds it: its value, or (is == 0) not there; the user's own, which come last, before the built-in */
static void arg(const char *line, const char *name, const char *is)
{
	char v[1024], own[1024];
	int found = xr_bootarg(line, name, v, sizeof(v));

	if (want.args && xr_bootarg(want.args, name, own, sizeof(own))) is = own;
	if (!is) CK(!found, "the command line has %s=%s, which nobody asked for", name, v);
	else CK(found && !strcmp(v, is), "the command line's %s is %s, not %s", name, found ? v : "not there", is);
}

/*
 * Everything XNU is handed at arm_init: the boot arguments at entered.arg, the device tree and the boot data they point
 * to, in the region (region.va, region.size) whose first image_end bytes are the image. Each line it prints is one
 * thing a reader in XNU (named in the comment) would not find as it expects it.
 */
static int boot_data(uint64_t image_end)
{
	const unsigned char *ba = (const unsigned char *)entered.arg, *virt = region.va, *dt;
	const uint64_t size = region.size;
	uint64_t phys, top, lo, fb;
	const struct xr_node *n, *c;
	const struct xr_prop *p;
	const char *line;
	char err[400], path[64];
	uint32_t dtlen;
	int i;

	bad = 0; nplaced = 0;
	CK(ba >= virt + image_end && ba + XR_BA_SIZE <= virt + size, "the boot arguments are not after the image, in the region");
	if (bad) return 1;

	/* boot_args (pexpert/pexpert/arm64/boot.h), as arm_init and arm_vm_init take them */
	CK(xr_ba16(ba, XR_BA_REVISION) == 2 && xr_ba16(ba, XR_BA_VERSION) == 2,
	    "boot_args Revision %u Version %u, not kBootArgsRevision2 and kBootArgsVersion2", xr_ba16(ba, XR_BA_REVISION), xr_ba16(ba, XR_BA_VERSION));
	phys = xr_ba64(ba, XR_BA_PHYSBASE);
	top = xr_ba64(ba, XR_BA_TOPOFKERNELDATA);
	lo = phys + image_end;
	CK(xr_ba64(ba, XR_BA_VIRTBASE) == (uint64_t)virt, "virtBase %#llx is not the region's address %p", X(xr_ba64(ba, XR_BA_VIRTBASE)), virt);
	CK(phys && !(phys & 0x3fff), "physBase %#llx is not on a 16 KB page", X(phys));
	CK(xr_ba64(ba, XR_BA_MEMSIZE) == size, "memSize %#llx is not the region's size %#llx", X(xr_ba64(ba, XR_BA_MEMSIZE)), X(size));
	CK(!xr_ba64(ba, XR_BA_MEMSIZEACTUAL) || xr_ba64(ba, XR_BA_MEMSIZEACTUAL) == size, "memSizeActual %#llx is not the region's size",
	    X(xr_ba64(ba, XR_BA_MEMSIZEACTUAL)));
	/* arm_vm_init: mem_size = ((gPhysBase + memSize) & ~PAGE_MASK) - gPhysBase, and a panic when it reaches half of the kernel's window */
	CK(((phys + size) & ~0xfffULL) - phys < (0xfffffffbffffffffULL - 0xffffffe000000000ULL) / 2,
	    "memSize %#llx: XNU's arm_vm_init panics (\"Unsupported memory configuration\") from "
	    "(VM_MAX_KERNEL_ADDRESS - VM_MIN_KERNEL_ADDRESS) / 2 = %#llx", X(size), X((0xfffffffbffffffffULL - 0xffffffe000000000ULL) / 2));
	CK(top > lo && top <= phys + size && !(top & 0x3fff), "topOfKernelData %#llx is not a 16 KB page after the image (%#llx), in the memory (to %#llx)",
	    X(top), X(lo), X(phys + size));
	CK(!xr_ba64(ba, XR_BA_BOOTFLAGS), "bootFlags %#llx (bit 0 is a dark boot)", X(xr_ba64(ba, XR_BA_BOOTFLAGS)));
	if (bad) return 1;
	place("boot_args", phys + (ba - virt), XR_BA_SIZE, lo, top);

	/* the device tree: deviceTreeP is its kernel virtual address (PE_init_platform hands it to SecureDTInit as it is) */
	dt = (const unsigned char *)xr_ba64(ba, XR_BA_DEVICETREEP);
	dtlen = xr_ba32(ba, XR_BA_DEVICETREELENGTH);
	if (!place("the device tree", phys + (dt - virt), dtlen, lo, top)) return 1;
	if (!(dt_root = xr_dt_parse(dt, dtlen, err, sizeof(err)))) { printf("test: the device tree: %s\n", err); return 1; }
	nodes(dt_root, "/");

	/* /: IODTPlatformExpert's names; SecureDTFindEntry("name", "device-tree") (PE_init_platform) */
	CK(xr_find(dt_root, "name", "device-tree") == dt_root, "the first node named \"device-tree\" is not the root");
	str(dt_root, "/", "model", want.model);
	if ((p = prop(dt_root, "/", "compatible", 0))) {	/* a list of strings: the platform's, then the model's */
		static const char own[] = "xnulinux,linux64";
		int listed = want.model[0] && strcmp(want.model, "xnulinux");
		unsigned long len = sizeof(own) + (listed ? strlen(want.model) + 1 : 0);

		CK(p->len == len && !memcmp(p->val, own, sizeof(own)) && (!listed || !strcmp((const char *)p->val + sizeof(own), want.model)),
		    "/ \"compatible\" (%u bytes) is not \"%s\"%s%s, each with its terminator", p->len, own, listed ? " then " : "", listed ? want.model : "");
	}

	/* /chosen */
	n = at("/chosen");
	u64(n, "/chosen", "dram-base", phys);		/* arm_init: gDramBase, 8 bytes */
	u64(n, "/chosen", "dram-size", size);
	if ((p = prop(n, "/chosen", "random-seed", 0))) {	/* bootseed_init asks PE_get_random_seed for 256 bytes, and all zero is no seed */
		for (i = 0; i < (int)p->len && !p->val[i]; i++) ;
		CK(p->len >= 256 && i < (int)p->len, "/chosen \"random-seed\": %u bytes%s; XNU's bootseed_init panics on fewer than 256, or on zeros", p->len,
		    i < (int)p->len ? "" : ", all zero");
	}
	CK(xr_is_str(prop(n, "/chosen", "firmware-version", 0)), "/chosen \"firmware-version\" is not a string");	/* PE_init_platform: iBoot_version */
	u32(n, "/chosen", "debug-enabled", 1);		/* PE_init_platform */
	prop(n, "/chosen", "boot-manifest-hash", 48);	/* IOGetBootManifestHash: a SHA-384 */
	if ((p = prop(n, "/chosen", "nvram-bank-size", 4))) {	/* IODTNVRAM: the bank's size, and that many bytes of image */
		const struct xr_prop *d = prop(n, "/chosen", "nvram-proxy-data", 0);

		CK(xr_u32(p, 0) && d && d->len >= xr_u32(p, 0), "/chosen \"nvram-proxy-data\" is %u bytes, \"nvram-bank-size\" %u", d ? d->len : 0, xr_u32(p, 0));
	}
	u32(n, "/chosen", "nvram-bank-count", 1);
	u32(n, "/chosen", "nvram-current-bank", 0);
	str(n, "/chosen", "crypto-hash-method", "sha2-384");
	prop(n, "/chosen", "chip-id", 4);		/* IOPlatformExpertDevice::generatePlatformUUID: 4 and 8 bytes */
	prop(n, "/chosen", "unique-chip-id", 8);
	prop(n, "/chosen", "board-id", 4);
	prop(n, "/chosen", "chip-epoch", 4);
	prop(n, "/chosen", "security-domain", 4);
	u32(n, "/chosen", "certificate-production-status", 0);	/* ml_device_is_prod_fused: 4 bytes */
	prop(n, "/chosen", "certificate-security-mode", 4);
	u32(n, "/chosen", "effective-production-status-ap", 0);	/* cpu_machine_idle_init: 4 bytes */
	prop(n, "/chosen", "effective-security-mode-ap", 4);
	prop(n, "/chosen", "mix-n-match-prevention-status", 4);
	if (want.root_uuid) {	/* IOFindBSDRoot: OSUnserializeXML of the value, a matching dictionary */
		snprintf(err, sizeof(err), "<dict><key>IOProviderClass</key><string>IOMedia</string><key>IOPropertyMatch</key>"
		    "<dict><key>UUID</key><string>%s</string></dict></dict>", want.root_uuid);
		str(n, "/chosen", "root-matching", err);
	} else none(n, "/chosen", "root-matching");
	for (i = 0; i < 2; i++) {	/* IOGetBootUUID, IOGetApfsPrebootUUID: at most a uuid_string_t (37 bytes), then uuid_parse */
		const char *name = i ? "apfs-preboot-uuid" : "boot-uuid", *is = i ? want.preboot_uuid : want.boot_uuid;

		if (!is) { none(n, "/chosen", name); continue; }
		str(n, "/chosen", name, is);
		if ((p = xr_prop(n, name)))
			CK(p->len <= 37 && xr_is_str(p) && is_uuid((const char *)p->val),
			    "/chosen \"%s\" (%u bytes) is not a UUID in a uuid_string_t: XNU does not take it", name, p->len);
	}

	/* /chosen/memory-map: the RAM disk (IOFindBSDRoot) and the static trust cache (load_static_trust_cache) */
	n = at("/chosen/memory-map");
	if (firmware[1].name && firmware[1].size) {
		if ((p = prop(n, "/chosen/memory-map", "RAMDisk", 16)) && place("the RAM disk", xr_u64(p, 0), xr_u64(p, 1), lo, top)) {
			/* mdevadd(-1, ml_static_ptovirt(base) >> 12, size >> 12, 0): the device is whole 4 KB pages */
			CK(!(xr_u64(p, 0) & 0xfff), "the RAM disk's address %#llx is not on a 4 KB page: md0 starts before it", X(xr_u64(p, 0)));
			CK(xr_u64(p, 1) >> 12 << 12 >= firmware[1].size,
			    "the RAM disk is %#lx bytes, md0 as XNU makes it from \"RAMDisk\" (size >> 12 pages) %#llx: its end is cut off", firmware[1].size,
			    X(xr_u64(p, 1) >> 12 << 12));
			CK(!memcmp(virt + (xr_u64(p, 0) - phys), firmware[1].data, firmware[1].size), "the RAM disk in XNU's memory is not the file");
		}
	} else none(n, "/chosen/memory-map", "RAMDisk");
	if (firmware[2].name && firmware[2].size) {
		if ((p = prop(n, "/chosen/memory-map", "TrustCache", 16)) && place("the trust cache", xr_u64(p, 0), xr_u64(p, 1), lo, top)) {
			const unsigned char *seg = virt + (xr_u64(p, 0) - phys);	/* trust_cache_offsets_t { num_caches, offsets[] }, then the modules */
			uint64_t len = xr_u64(p, 1);

			CK(len >= 8 && xr_ba32(seg, 0) == 1 && xr_ba32(seg, 4) >= 8 && xr_ba32(seg, 4) <= len && len - xr_ba32(seg, 4) >= firmware[2].size &&
			    !memcmp(seg + xr_ba32(seg, 4), firmware[2].data, firmware[2].size),
			    "the trust cache segment (%#llx bytes: %u caches, the first at %#x) does not hold the file's %#lx bytes as its one module",
			    X(len), len >= 8 ? xr_ba32(seg, 0) : 0, len >= 8 ? xr_ba32(seg, 4) : 0, firmware[2].size);
		}
	} else none(n, "/chosen/memory-map", "TrustCache");
	at("/chosen/manifest-properties");
	n = at("/chosen/asmb");		/* csr_bootstrap: _csr_get_dt_uint64 and _csr_get_dt_bool panic on another size */
	prop(n, "/chosen/asmb", "lp-sip0", 8);
	prop(n, "/chosen/asmb", "lp-sip1", 4);

	/* /cpus: ml_parse_cpu_topology, pe_identify_machine, kexts/XnuCPU */
	n = at("/cpus");
	if (n) {
		unsigned running = 0, slots[256] = { 0 };

		CK((int)n->nchildren == want.cpus && want.cpus <= 32, "/cpus has %u CPUs, not %d (XNU's tables hold 32)", n->nchildren, want.cpus);
		for (c = n->child, i = 0; c; c = c->next, i++) {
			uint32_t reg = (uint32_t)((host_mpidr ? host_mpidr(i) : (fki_u64)i) & 0xffff);

			snprintf(path, sizeof(path), "/cpus/cpu%d", i);
			str(c, path, "name", path + 6);
			str(c, path, "compatible", "xnulinux,cpu");	/* XnuCPU's IONameMatch */
			str(c, path, "state", i ? "waiting" : "running");	/* ml_is_boot_cpu: one boot CPU */
			running += xr_str_eq(xr_prop(c, "state"), "running");
			u32(c, path, "reg", reg);			/* its MPIDR's Aff1:Aff0: ml_get_cpu_number; XnuCPU reads 4 bytes */
			CK(!slots[(reg & 0xf) | (reg >> 4 & 0xf0)]++, "%s: its slot in XNU's per-CPU arrays (LINUX64_CPU_SLOT) is another CPU's too", path);
			u64(c, path, "timebase-frequency", host_hz);	/* pe_identify_machine: 8 bytes, or 4 */
			u32(c, path, "cluster-type", want.ecpus >> i & 1 ? 'E' : 'P');
			p = xr_prop(c, "device_type");
			CK(!p || !xr_str_eq(p, "cpu"), "%s: device_type \"cpu\" is Apple's CPU driver's", path);
		}
		CK(running == 1, "%u CPUs are \"running\": ml_parse_cpu_topology wants one boot CPU", running);
	}

	/* /defaults (IODeviceTreeAlloc, the TUNABLE_DTs), /options (IODTNVRAM), /product (PE_init_platform, hw.optional's brand string) */
	at("/defaults");
	at("/options");
	n = at("/product");
	CK(xr_find(dt_root, "name", "product") == n, "the first node named \"product\" is not /product");
	str(n, "/product", "unique-model", want.model);
	str(n, "/product", "sub-product-type", want.model);
	CK(xr_is_str(prop(n, "/product", "product-name", 0)), "/product \"product-name\" is not a string");
	if (want.soc) str(n, "/product", "product-soc-name", want.soc); else none(n, "/product", "product-soc-name");
	/* /sgx: kexts/XnuCpuMetal matches "xnulinux,gpu" and takes a "model" of up to 64 bytes with its terminator */
	n = at("/sgx");
	str(n, "/sgx", "compatible", "xnulinux,gpu");
	if (want.gpu) {
		str(n, "/sgx", "model", want.gpu);
		CK(strlen(want.gpu) < 64, "/sgx \"model\" is %lu bytes: XnuCpuMetal takes 64 at most, and names the GPU itself otherwise", strlen(want.gpu) + 1);
	} else none(n, "/sgx", "model");
	/* /arm-io: pe_arm_get_soc_base_phys (the first node of that name; device_type a string; ranges' second cell the SoC's base, not 0) */
	n = at("/arm-io");
	CK(xr_find(dt_root, "name", "arm-io") == n, "the first node named \"arm-io\" is not /arm-io");
	CK(xr_is_str(p = prop(n, "/arm-io", "device_type", 0)) && p->len <= 32, "/arm-io \"device_type\" is not a string of 32 bytes at most");
	if ((p = prop(n, "/arm-io", "ranges", 0)))
		CK(p->len >= 16 && !(p->len & 7) && xr_u64(p, 1), "/arm-io \"ranges\" (%u bytes): no 64-bit cells with a SoC base that is not 0", p->len);

	/* Video (PE_init_platform, kexts/XnuDisplay) and /arm-io/gpu, the display's node */
	fb = xr_ba64(ba, XR_BA_V_BASEADDR);
	c = n ? xr_lookup(n, "gpu") : 0;
	if (want.fb_w > 0 && want.fb_h > 0) {
		const uint64_t bytes = (uint64_t)want.fb_w * want.fb_h * 4;

		CK(xr_ba64(ba, XR_BA_V_WIDTH) == (uint64_t)want.fb_w && xr_ba64(ba, XR_BA_V_HEIGHT) == (uint64_t)want.fb_h &&
		    xr_ba64(ba, XR_BA_V_ROWBYTES) == (uint64_t)want.fb_w * 4 && xr_ba64(ba, XR_BA_V_DEPTH) == 32 && !xr_ba64(ba, XR_BA_V_DISPLAY),
		    "Video is %llu x %llu, %llu bytes a row, depth %#llx, display %llu: not %d x %d at 32 bits", X(xr_ba64(ba, XR_BA_V_WIDTH)),
		    X(xr_ba64(ba, XR_BA_V_HEIGHT)), X(xr_ba64(ba, XR_BA_V_ROWBYTES)), X(xr_ba64(ba, XR_BA_V_DEPTH)), X(xr_ba64(ba, XR_BA_V_DISPLAY)),
		    want.fb_w, want.fb_h);
		if (place("the framebuffer", fb, bytes, lo, top)) {
			const unsigned char *px = virt + (fb - phys);
			uint64_t k;

			CK(!(fb & 0xfff), "the framebuffer's address %#llx is not on a page", X(fb));
			for (k = 0; k < bytes && px[k] == ((k & 3) == 3 ? 0xff : 0); k++) ;
			CK(k == bytes, "the framebuffer is not opaque black at byte %#llx", X(k));
		}
		CK(c && xr_str_eq(xr_prop(c, "compatible"), "paravirtualizedgraphics,gpu"),
		    "no /arm-io/gpu compatible \"paravirtualizedgraphics,gpu\" for the framebuffer");
	} else {
		for (i = XR_BA_V_BASEADDR; i <= XR_BA_V_DEPTH; i += 8) CK(!xr_ba64(ba, i), "no framebuffer was asked for, and Video is not zero at %d", i);
		CK(!c, "no framebuffer was asked for, and there is an /arm-io/gpu");
	}

	/* the command line, as PE_parse_boot_argn reads it */
	line = (const char *)ba + XR_BA_COMMANDLINE;
	CK(memchr(line, 0, XR_BOOT_LINE_LENGTH) != 0, "the command line is not terminated in its %d bytes", XR_BOOT_LINE_LENGTH);
	if (!bad || memchr(line, 0, XR_BOOT_LINE_LENGTH)) {
		arg(line, "-v", "1");
		arg(line, "-noprogress", "1");
		arg(line, "serial", "0");
		arg(line, "startup_debug", "1");
		arg(line, "debug", "0x8");
		arg(line, "linux64_vm16k", want.vm16k ? "1" : 0);	/* arm_init: 16 KB VM pages */
		if (xr_bootarg(line, "vm_compression_limit", err, sizeof(err)))	/* in VM pages */
			CK(strtoull(err, 0, 10) * (want.vm16k ? 16384 : 4096) == 6ULL << 30, "vm_compression_limit=%s is not 6 GB of %d KB pages", err,
			    want.vm16k ? 16 : 4);
		else CK(0, "the command line has no vm_compression_limit");
		arg(line, "rd", want.md0 ? "md0" : 0);			/* IOFindBSDRoot */
		arg(line, "ml-timeout-debug-ack-scale", "100");
		arg(line, "droidvm-timebase-hz", host_hz != 24000000 ? "24000000" : 0);	/* pe_identify_machine: the counter is scaled to 24 MHz */
		if (want.args) {
			unsigned long l = strlen(line), a = strlen(want.args);

			CK(l > a && !strcmp(line + l - a, want.args) && line[l - a - 1] == ' ', "the command line does not end with the bootargs asked for: \"%s\"", line);
		}
	}
	xr_dt_free((struct xr_node *)dt_root); dt_root = 0;
	return bad;
}

/* ---- the cases ---- */

/* insmod: xnu_init() returns `want`; then, unloaded again (or the load having failed), the module holds nothing */
static int insmod(int want, int (*loaded)(void))
{
	int err = xnu_init(), bad = err != want;

	if (bad) printf("test: xnu_init returned %d, not %d\n", err, want);
	if (!err && loaded) bad |= loaded();
	if (!err) xnu_exit();
	if (held) { printf("test: %d regions or firmware files still held\n", held); bad = 1; }
	return bad;
}

static int rebased(void)
{
	const u64_t *d = (const u64_t *)((const char *)img.region.va + FIXUP), want = TARGET + img.slide;
	int bad = d[0] != want || d[1] != want || d[(PG - 0x100) / 8 - 1] != want;

	if (bad) printf("test: the three pointers are not TARGET's address in the region\n");
	return bad;
}
static int c_defaults(void) { kernel(KV + 0x1000, FIXUP); return insmod(0, rebased); }

/* The boot data build_boot puts after the image (boot args, device tree, RAM disk, trust cache, framebuffer) is in the region */
static int c_ram4(void) { kernel(KV + 0x1000, FIXUP); ram_mb = 4; return insmod(0, 0); }	/* 4 MB of framebuffer by default */
static int c_ramdisk(void)
{
	kernel(KV + 0x1000, FIXUP);
	ramdisk = "rd"; firmware[1].name = ramdisk; firmware[1].size = 9 << 20; firmware[1].data = calloc(1, 9 << 20);
	ram_mb = 8; fb_width = 0;
	return insmod(0, 0);
}
static int c_mode0(void) { kernel(KV + 0x1000, FIXUP); mode = 0; return insmod(0, 0); }
static int c_wrap(void)		/* 16 EiB of framebuffer and 16 GiB of RAM: the sum is 400 KB */
{
	kernel(KV + 0x1000, FIXUP); fb_width = fb_height = 0x7fffffff; ram_mb = 16384;
	return insmod(-22, 0);
}

/* A bare kernel's threaded fixups stay inside the image: -ENOEXEC otherwise */
static int c_fixup(void)	/* at the region's end: the image, the boot data's room, 1 MB */
{
	kernel(KV + 0x1000, 3 * PG + 0x60000 + (1 << 20)); ram_mb = 1; fb_width = 0;
	return insmod(-8, 0);
}
static int c_starts(void) { kernel(0x4141414141414000ULL, FIXUP); return insmod(-8, 0); }

/* The load commands slid are the loaded ones, at the address a __TEXT of file offset 0 names: inside the image as well */
static int c_overlap(void)	/* a segment laid over __TEXT's command in memory gives it 4 billion sections */
{
	struct seg64 *s;

	kernel(KV + 0x1000, FIXUP);
	s = seg("__OVER", 2 * PG + 0x200, 4, 1);
	s->vmaddr = KV + sizeof(struct mh64) + __builtin_offsetof(struct seg64, nsects);
	*(u32_t *)(file + 2 * PG + 0x200) = 0xffffffff;
	return insmod(-8, 0);
}
static int c_header(void)	/* a second, empty __TEXT: the header's address is its, anywhere (and no fixup to refuse first) */
{
	kernel(KV + 0x1000, 0xffffffff);
	seg("__TEXT", 0, 0, 5)->vmaddr = 0x4141414141414000ULL;
	return insmod(-8, 0);
}

/*
 * What an EL0 host's load does to a collection (el0_patch_image) whose corecrypto does not have the two ID register
 * reads it expects, loaded a second time with el0_fips_sha256 set to the digest the first load logged: the line that
 * says why the integrity check stays. (Mode 3 itself needs the kernel's addresses: the image is parsed and placed
 * here, as run_m2b_el0 does it.)
 */
static int c_fips(void)
{
	unsigned long n = collection();
	void *at;

	if (macho_load(&img, memcpy(malloc(n), file, n), n, ~0UL)) return 1;
	at = calloc(1, img.vmhigh - img.vmlow);
	img.slide = (u64_t)at - img.vmlow;
	for (int load = 0; load < 2; load++) {
		if (macho_copy(&img, at)) return 1;
		el0_patch_image(&img);
		el0_fips_sha256 = cc_sha;
	}
	return strcmp(last_log, "xnu: preserving _fipspost_post_integrity (unexpected corecrypto ubsan patches) 0x0000000000000000") != 0;
}
static int c_logkv(void)	/* a key longer than the line is cut: 108 characters of it, then the value */
{
	char key[200];

	memset(key, 'k', sizeof(key) - 1); key[sizeof(key) - 1] = 0;
	logkv(key, 1);
	return strlen(last_log) != 108 + 3 + 16;
}

/* bootargs longer than what XNU's command line has left were dropped whole, in silence, and the system started without them */
static int c_bootargs_long(void)
{
	static char args[901];

	memset(args, 'a', sizeof(args) - 1);
	kernel(KV + 0x1000, FIXUP); bootargs = args;
	if (insmod(-22, 0)) return 1;
	if (strncmp(last_log, "xnu: bootargs do not fit in XNU's command line", 46)) { printf("test: the last log line is \"%s\"\n", last_log); return 1; }
	return 0;
}
static int c_bootargs(void) { kernel(KV + 0x1000, FIXUP); bootargs = "-v debug=0x14e serial=3"; return insmod(0, rebased); }

/* ---- what XNU is handed (boot_data), for what a user can ask of the module ---- */

/*
 * Mode 2 after xnu_init: XNU was entered once, at _arm_init's address in the region, with boot arguments that read
 * back as asked; the kernel's linux64_* variables are set; and its load commands are slid (arm_init takes the image
 * for loaded and slid: each segment's and section's address, nothing else of the header).
 */
static int entered_right(void)
{
	const unsigned char *at = region.va;
	const uint64_t slide = (uint64_t)at - KV;
	const int el0 = !!(host_caps & FKI_CAP_EL0);
	unsigned char hdr[sizeof(struct mh64) + 1024];
	const struct mh64 *h = (const void *)hdr;
	struct lc *l = (void *)(hdr + sizeof(*h));
	int r = boot_data(3 * PG);
	const uint64_t *v64 = (const void *)(at + 2 * PG);	/* the kernel's linux64_hook and linux64_slide, */
	const uint32_t *v32 = (const void *)(at + 2 * PG + 16);	/* its linux64_host_abi_version, linux64_el0 and linux64_el0_caps */

	bad = 0;
	CK(entered.calls == 1 && entered.fn == TARGET + slide, "XNU was entered %d times, at %#lx: _arm_init is at %#llx", entered.calls, entered.fn,
	    X(TARGET + slide));
	CK(v64[0] == (uint64_t)(el0 ? xnu_world_upcall_el0 : xnu_world_upcall), "linux64_hook is not the module's upcall for this host");
	CK(v64[1] == slide, "linux64_slide is %#llx, the image slid by %#llx", X(v64[1]), X(slide));
	CK(v32[0] == LINUX64_ABI_VERSION, "linux64_host_abi_version is %u", v32[0]);
	CK(v32[2] == (uint32_t)el0, "linux64_el0 is %u on a host that %s", v32[2], el0 ? "runs XNU at EL0" : "does not");
	CK(v32[3] == (el0 && host_caps & FKI_CAP_EXEC_ONLY ? LINUX64_EL0_CAP_EXEC_ONLY : 0), "linux64_el0_caps is %#x", v32[3]);
	memcpy(hdr, file, sizeof(hdr));
	for (u32_t i = 0; i < h->ncmds; i++, l = (void *)((char *)l + l->cmdsize)) {
		struct seg64 *s = (void *)l;

		if (l->cmd != LC_SEGMENT_64 || !s->vmsize) continue;
		s->vmaddr += slide;
		for (u32_t k = 0; k < s->nsects; k++) ((struct sect64 *)(s + 1))[k].addr += slide;
	}
	CK(!memcmp(at, hdr, sizeof(*h) + h->sizeofcmds), "the loaded Mach-O header is not the file's with its segments' and sections' addresses slid");
	return r | bad;
}
static int handed(void) { return entered_right() | rebased(); }	/* and the default image's fixups are applied */
static void small(void) { kernel(KV + 0x1000, FIXUP); ram_mb = 8; }	/* the default image, in little memory */

static int c_handed(void) { small(); return insmod(0, handed); }
static int c_handed_256(void) { kernel(KV + 0x1000, FIXUP); return insmod(0, handed); }	/* ram_mb as it is by default */

/* a chain with an authenticated pointer (its target counted from the Mach-O header; no signature is made in user space) and a bind, which stays */
static int chained(void)
{
	const u64_t *d = (const u64_t *)(region.va + FIXUP), slide = (u64_t)region.va - KV;

	if (d[0] == KV + PG + slide && d[1] == (1ULL << 62 | 2ULL << 51 | 0x55) && d[2] == TARGET + slide) return entered_right();
	printf("test: the chain's three are %#llx %#llx %#llx\n", X(d[0]), X(d[1]), X(d[2]));
	return 1;
}
static int c_chain(void)
{
	small();
	*(u64_t *)(file + FIXUP) = 1ULL << 63 | 2ULL << 51 | 2ULL << 49 | 1ULL << 48 | 0x1234ULL << 32 | PG;
	*(u64_t *)(file + FIXUP + 16) = *(u64_t *)(file + FIXUP + 8);
	*(u64_t *)(file + FIXUP + 8) = 1ULL << 62 | 2ULL << 51 | 0x55;
	return insmod(0, chained);
}

/* CPUs: Linux's 0 .. cpus-1, as many as the host has and XNU's tables hold, each with a slot of its own */
static int c_cpus4(void) { small(); cpus = want.cpus = 4; ecpus = want.ecpus = 0xa; return insmod(0, handed); }
static int c_cpus0(void) { small(); cpus = 0; return insmod(0, handed); }
static int c_cpus_neg(void) { small(); cpus = -3; ecpus = want.ecpus = 1; return insmod(0, handed); }	/* the one CPU an efficiency core */
static int c_cpus_host(void) { small(); cpus = 12; want.cpus = 4; ecpus = ~0UL; want.ecpus = 0xf; return insmod(0, handed); }
static fki_u64 mpidr_aff1(int cpu) { return 0x80000000ULL | (fki_u64)cpu << 8; }	/* a core a cluster, as on Qualcomm's and Arm's own SoCs */
static int c_cpus_aff1(void) { small(); host_mpidr = mpidr_aff1; cpus = want.cpus = 3; return insmod(0, handed); }
static fki_u64 mpidr_16(int cpu) { return 0x80000000ULL | (fki_u64)(cpu >> 4) << 8 | (cpu & 15); }	/* 16 to a cluster */
static int c_cpus_max(void)
{
	small(); host_cpus = 64; host_mpidr = mpidr_16; cpus = 64; want.cpus = 32; ecpus = want.ecpus = 0xffff0000UL;
	return insmod(0, handed);
}
static fki_u64 mpidr_dup(int cpu) { return cpu == 2 ? 0x11 : (fki_u64)cpu; }	/* Aff0 17: CPU 1's slot (the low 4 bits) */
static int c_cpus_slot(void) { small(); host_mpidr = mpidr_dup; cpus = 4; want.cpus = 2; return insmod(0, handed); }

/* the names: the model (the product type), the chip's and the GPU's for System Information */
static int c_names(void)
{
	small(); model = (char *)(want.model = "Mac16,10"); soc_name = (char *)(want.soc = "Cortex-X925"); gpu_model = (char *)(want.gpu = "NVIDIA GB10");
	return insmod(0, handed);
}
static int c_names_empty(void) { small(); soc_name = ""; gpu_model = ""; return insmod(0, handed); }	/* a script's unset variables: none */
static int c_model_own(void) { small(); model = (char *)(want.model = "xnulinux"); return insmod(0, handed); }	/* the platform's own name: once */
static char longname[400000];
static int c_model_78(void)	/* with the platform's name, the 96 bytes the module has for the list */
{
	small(); memset(longname, 'M', 78); model = (char *)(want.model = longname);
	return insmod(0, handed);
}
static int c_name_huge(void)	/* a name the device tree's 256 KB have no room for */
{
	small(); memset(longname, 'S', sizeof(longname) - 1); soc_name = longname;
	if (insmod(-22, 0)) return 1;
	return strcmp(last_log, "xnu: ADT overflow") != 0;
}

/* the root: by UUID (root-matching), APFS's boot volume and Preboot folder (boot-uuid, apfs-preboot-uuid) */
#define UUID1 "0F9D5B7A-3C41-4E0B-9A6D-52E1C7B8A930"
#define UUID2 "6b1e2c3d-4f50-4a61-8b72-93a4b5c6d7e8"
static int c_uuids(void)
{
	small(); root_uuid = (char *)(want.root_uuid = UUID1); boot_uuid = (char *)(want.boot_uuid = UUID2);
	preboot_uuid = (char *)(want.preboot_uuid = UUID2);
	return insmod(0, handed);
}
static int c_uuids_empty(void) { small(); root_uuid = ""; boot_uuid = ""; preboot_uuid = ""; return insmod(0, handed); }	/* unset variables: none */
static int refused_uuid(void)	/* (where it loads all the same: what XNU would find of it) */
{
	if (insmod(-22, handed)) return 1;
	if (!strstr(last_log, "UUID")) { printf("test: the last log line does not say why: \"%s\"\n", last_log); return 1; }
	return 0;
}
static int c_root_uuid_short(void) { small(); root_uuid = (char *)(want.root_uuid = "0F9D5B7A-3C41-4E0B-9A6D-52E1C7B8A93"); return refused_uuid(); }
static int c_boot_uuid_long(void) { small(); boot_uuid = (char *)(want.boot_uuid = UUID2 "-0001"); return refused_uuid(); }
static int c_preboot_uuid_bad(void)
{
	small(); preboot_uuid = (char *)(want.preboot_uuid = "6b1e2c3d-4f50-4a61-8b72-93a4b5c6d7eZ");
	return refused_uuid();
}

/* the RAM disk and the static trust cache: in XNU's memory, described in /chosen/memory-map; rd=md0 */
static unsigned char *bytes(unsigned long n, unsigned seed)
{
	unsigned char *b = malloc(n);

	for (unsigned long i = 0; i < n; i++) b[i] = (unsigned char)((seed = seed * 1103515245 + 12345) >> 16) | 1;	/* no byte 0 */
	return b;
}
static void blobs(unsigned long rd, unsigned long tc)
{
	if (rd) { ramdisk = "rd"; firmware[1].name = ramdisk; firmware[1].size = rd; firmware[1].data = bytes(rd, 1); want.md0 = 1; }
	if (tc) { trustcache = "tc"; firmware[2].name = trustcache; firmware[2].size = tc; firmware[2].data = bytes(tc, 2); }
}
static int c_blobs(void) { small(); blobs(64 << 10, 5000); return insmod(0, handed); }
static int c_rd_512(void) { small(); blobs(137 * 512, 0); return insmod(0, handed); }	/* a disk image of whole sectors, not of whole 4 KB pages */
static int c_tc_only(void) { small(); blobs(0, 16384 - 8); fb_width = want.fb_w = 0; return insmod(0, handed); }	/* with its header, 16 KB exactly */
static int c_hostdisk(void) { small(); host_files = 1; hostdisk = "/var/tmp/root.img"; want.md0 = 1; return insmod(0, handed); }

/* what a load is refused for, with nothing left held: no image; a kernel without the hook; one built for another linux64_abi.h */
static int c_no_image(void) { small(); blobs(4096, 4096); firmware[0].name = "elsewhere"; return insmod(-2, 0); }
static int c_no_hook(void)
{
	small();
	for (char *n = (char *)file + st->stroff; n < (char *)file + st->stroff + st->strsize; n += strlen(n) + 1)
		if (!strcmp(n, "_linux64_hook")) n[1] = 'L';
	return insmod(-2, 0);
}
static int c_abi(void)
{
	small(); *(u32_t *)(file + PG + 0x100) = LINUX64_ABI_VERSION + 1;
	return insmod(-22, 0) || strncmp(last_log, "xnu: LINUX64_ABI_VERSION expected by host", 41) != 0;
}

/* the framebuffer; the VM page size; the counter's rate; the user's boot-args */
static int c_fb_none(void) { small(); fb_height = want.fb_h = 0; return insmod(0, handed); }
static int c_fb_odd(void) { small(); fb_width = want.fb_w = 1366; fb_height = want.fb_h = 767; return insmod(0, handed); }
static int c_fb_neg(void) { small(); fb_width = want.fb_w = -1280; return insmod(0, handed); }
static int c_vm4k(void) { small(); vm16k = want.vm16k = 0; return insmod(0, handed); }
static int c_hz(void) { small(); host_hz = 1000000000; return insmod(0, handed); }	/* Armv8.6's 1 GHz counter */
static int c_args(void)
{
	small(); bootargs = (char *)(want.args = "amfi_get_out_of_my_way=1 debug=0x14e serial=3 lp-sip0=2050");
	return insmod(0, handed);
}
static int c_args_edge(void)	/* the longest bootargs that fit: the line's 1023 characters used to the last */
{
	static char a[1024];
	int err;

	small();
	memset(a, 'a', sizeof(a) - 1);
	for (int n = 1023; n > 800; n--) {	/* the first length the module takes */
		a[n] = 0; bootargs = (char *)(want.args = a);
		if ((err = xnu_init()) == 0) {
			unsigned long len = strlen((const char *)entered.arg + XR_BA_COMMANDLINE);
			int r = handed() || len != XR_BOOT_LINE_LENGTH - 1;

			if (r) printf("test: bootargs of %d characters were taken, and the command line is %lu long\n", n, len);
			xnu_exit();
			return r || held;
		}
		if (err != -22 || held) { printf("test: xnu_init returned %d for bootargs of %d characters (%d held)\n", err, n, held); return 1; }
	}
	return 1;
}

/* an EL0 host (FKI_CAP_EL0): the kernel is told so, its upcall is the EL0 one, and its EL1-only instructions are gone */
static int el0_words(void)
{
	const u32_t *w = (const u32_t *)(region.va + PG + 0x40);
	int r = handed();

	if (w[0] == 0xd503201f && w[1] == 0xd2800009) return r;
	printf("test: the kernel's brk #0xff00 and mrs x9, ID_AA64PFR0_EL1 are %#x and %#x on an EL0 host\n", w[0], w[1]);
	return 1;
}
static int c_el0_handed(void)
{
	small(); host_caps = FKI_CAP_EL0 | FKI_CAP_PAC | FKI_CAP_EXEC_ONLY;
	*(u32_t *)(file + PG + 0x40) = 0xd43fe000; *(u32_t *)(file + PG + 0x44) = 0xd5380409;	/* brk #0xff00; mrs x9, ID_AA64PFR0_EL1 */
	return insmod(0, el0_words);
}
static int c_el0_nopac(void) { small(); host_caps = FKI_CAP_EL0; return insmod(-95, 0); }

/* ram_mb: XNU's memory after the image and the boot data, as much as XNU can manage */
static int c_ram_bad(void)
{
	static const int no[] = { 0, -1, 65537, 0x7fffffff, (int)0x80000000 };

	for (unsigned i = 0; i < sizeof(no) / sizeof(no[0]); i++) {
		small(); ram_mb = no[i];
		if (insmod(-22, 0)) { printf("test: (ram_mb=%d)\n", no[i]); return 1; }
	}
	return 0;
}
static int c_ram_55g(void) { small(); ram_mb = 55 << 10; host_head = 8 << 20; return insmod(0, handed); }
static int said_ram(void)
{
	if (!strstr(last_log, "ram_mb")) { printf("test: the last log line does not name ram_mb: \"%s\"\n", last_log); return 1; }
	return 0;
}
static int c_ram_56g(void)	/* arm_vm_init's limit: half of the kernel's address window */
{
	small(); ram_mb = 56 << 10; host_head = 8 << 20;
	return insmod(-22, handed) || said_ram();
}
static int c_ram_56g_less(void)	/* 1 MB below it: over it with the image and the boot data */
{
	small(); ram_mb = (56 << 10) - 1; host_head = 8 << 20;
	return insmod(-22, handed) || said_ram();
}

/*
 * Mode 3's boot data: build_boot as run_m2b and run_m2b_el0 call it, for an image at its link address in a window that
 * starts on the 2 MiB boundary below it, on memory with a physical address of its own. (Not through xnu_init: the window
 * is at the kernel's own addresses.) The memory behind it is what mode 3 asks the host for without ram_mb: the image, the
 * boot data's room and what the RAM disk, the trust cache and the framebuffer take, each in whole 16 KB pages.
 */
static unsigned long pages16k(unsigned long n) { return (n + 0x3fff) & ~0x3fffUL; }
static int c_mode3_data(void)
{
	const unsigned long image_off = KV & 0x1fffff, image_size = 3 * PG;
	unsigned long head;
	struct boot_args_v2 *ba;

	blobs(3 * 4096, 777); cpus = want.cpus = 2;
	rd_data = firmware[1].data; rd_size = firmware[1].size; tc_data = firmware[2].data; tc_size = firmware[2].size;	/* as xnu_init reads them */
	head = image_off + image_size + 0x60000 + pages16k(rd_size) + pages16k(tc_size + 8) + pages16k(1280 * 800 * 4);
	region.va = calloc(1, head); region.size = head + (2048UL << 20);
	ba = build_boot(region.va, 0x8c0000000ULL, region.size, image_off, image_size);
	entered.arg = (unsigned long)ba;
	if (!ba || xr_ba64(ba, XR_BA_PHYSBASE) != 0x8c0000000ULL) { printf("test: build_boot: no boot arguments for memory at 0x8c0000000\n"); return 1; }
	return boot_data(image_off + image_size);
}

/* mode 3 through xnu_init, as far as its memory: what it asks the host for (the test's host has none) */
static int mode3(int el0, int mb, int err, u64_t ask)
{
	unsigned long n = collection();

	firmware[0].name = image; firmware[0].data = file; firmware[0].size = n;
	mode = 3; ram_mb = mb; fb_width = 0; asked = 0;
	if (el0) host_caps = FKI_CAP_EL0 | FKI_CAP_PAC;
	if (insmod(err, 0)) return 1;
	if (asked != ask) { printf("test: mode 3 asked the host for %#llx bytes, not %#llx\n", X(asked), X(ask)); return 1; }
	return 0;
}
#define M3_HEAD ((KV & 0x1fffff) + 4 * PG + 0x60000)	/* the collection in its window, the boot data's room */
static int c_mode3_ask(void) { return mode3(0, 4096, -12, M3_HEAD + (4096ULL << 20)); }
static int c_mode3_ask_el0(void) { return mode3(1, 4096, -12, M3_HEAD + (4096ULL << 20)); }
static int c_mode3_56g(void) { return mode3(0, 56 << 10, -22, 0) || said_ram(); }
static int c_mode3_56g_el0(void) { return mode3(1, 56 << 10, -22, 0) || said_ram(); }
static int c_mode3_55g(void) { return mode3(0, 55 << 10, -12, M3_HEAD + (55ULL << 30)); }

/* ---- a kernel collection: its entries, its symbols, where it is placed, and what an EL0 host patches in it ---- */

/*
 * A collection whose kernel and corecrypto have code of their own. The words an EL0 host must change (a UBSan trap to
 * a nop, a read of ID_AA64ISAR0_EL1 or ID_AA64PFR0_EL1 to a move of 0) and, next to them, the ones it must not: other
 * traps and ID registers, the same words in sections that are not code, in a segment that is not executable, and right
 * outside a code section. corecrypto has two ID register reads and _fipspost_post_integrity. Returns the file's size.
 */
#define BRK 0xd43fe000u		/* brk #0xff00 */
#define NOP 0xd503201fu
static const struct { unsigned long off; u32_t before, after; } words[] = {
	/* the kernel's __TEXT_EXEC,__text: 4 * PG + 0x100, 0x200 bytes */
	{ 4 * PG + 0xfc, BRK, BRK },			/* the word before the section */
	{ 4 * PG + 0x100, BRK, NOP },			/* its first */
	{ 4 * PG + 0x104, 0xd4200000 | 0xff01 << 5, 0xd4200000 | 0xff01 << 5 },	/* brk #0xff01 */
	{ 4 * PG + 0x108, 0xd5380600 | 3, 0xd2800000 | 3 },	/* mrs x3, ID_AA64ISAR0_EL1 */
	{ 4 * PG + 0x10c, 0xd5380400 | 30, 0xd2800000 | 30 },	/* mrs x30, ID_AA64PFR0_EL1 */
	{ 4 * PG + 0x110, 0xd5380620, 0xd5380620 },		/* mrs x0, ID_AA64ISAR1_EL1 */
	{ 4 * PG + 0x114, 0xd5380700, 0xd5380700 },		/* mrs x0, ID_AA64MMFR0_EL1 */
	{ 4 * PG + 0x118, 0xd5180600, 0xd5180600 },		/* msr ID_AA64ISAR0_EL1, x0 (not a read) */
	{ 4 * PG + 0x2fc, BRK, NOP },			/* its last */
	{ 4 * PG + 0x300, BRK, BRK },			/* the word after it */
	{ 4 * PG + 0x400, BRK, BRK },			/* __TEXT_EXEC,__const: not code */
	{ 4 * PG + 0x404, 0xd5380601, 0xd5380601 },
	{ 4 * PG + 0x500, BRK, NOP },			/* __TEXT_EXEC,__stubs: S_ATTR_SOME_INSTRUCTIONS */
	{ 6 * PG + 0x100, BRK, BRK },			/* __DATA,__text: flagged as code in a segment that is not executable */
	{ 6 * PG + 0x104, 0xd5380400, 0xd5380400 },
	/* corecrypto's __TEXT_EXEC,__text: 5 * PG, a page */
	{ 5 * PG + 0x10, 0xd5380600 | 8, 0xd2800000 | 8 },
	{ 5 * PG + 0x20, 0xd5380400 | 9, 0xd2800000 | 9 },
	{ 5 * PG + 0x80, 0xd503237f, 0xd503237f },		/* _fipspost_post_integrity: pacibsp, */
	{ 5 * PG + 0x84, 0xa9bf7bfd, 0xa9bf7bfd },		/* stp x29, x30, [sp, #-16]!, */
	{ 5 * PG + 0x88, 0x910003fd, 0x910003fd },		/* mov x29, sp */
};
#define FIPS (5 * PG + 0x80)
static unsigned long kc(void)
{
	struct seg64 *s;

	memset(file, 0, sizeof(file));
	header(0, MH_FILESET);
	seg("__TEXT", 0, 4 * PG, 1);
	seg("__TEXT_EXEC", 4 * PG, 2 * PG, 5);
	seg("__DATA", 6 * PG, PG, 3);
	entry("com.apple.kernel", PG);
	entry("com.apple.kec.corecrypto", 2 * PG);
	entry("com.apple.kernel.not", 3 * PG);		/* names that only begin, or go on, as the two the module looks for */
	header(3 * PG, 11);
	header(PG, 2);
	seg("__TEXT", PG, PG, 1);
	s = seg("__TEXT_EXEC", 4 * PG, PG, 5);
	sect(s, "__text", KV + 4 * PG + 0x100, 0x200, S_ATTR_PURE_INSTRUCTIONS);
	sect(s, "__const", KV + 4 * PG + 0x400, 0x100, 0);
	sect(s, "__stubs", KV + 4 * PG + 0x500, 0x10, S_ATTR_SOME_INSTRUCTIONS);
	sect(seg("__DATA", 6 * PG, PG, 3), "__text", KV + 6 * PG + 0x100, 0x100, S_ATTR_PURE_INSTRUCTIONS);
	symtab(7 * PG);
	sym("_arm_init_tramp", KV + 4 * PG + 0x200);
	sym("_arm_init", KV + 4 * PG + 0x100);
	sym("_arm", KV + 4 * PG + 0x280);
	sym("_linux64_hook", KV + 6 * PG);
	sym("_outside", KV + 9 * PG);			/* a value past the image */
	sym("_undefined", KV + 6 * PG + 8);
	((struct nlist64 *)(file + st->symoff))[st->nsyms - 1].n_type = 0x01;	/* N_UNDF | N_EXT */
	sym("_last", KV + 7 * PG - 1);			/* the image's last byte */
	{
		const unsigned long end = st->stroff + st->strsize;

		header(2 * PG, 11);	/* MH_KEXT_BUNDLE */
		seg("__TEXT", 2 * PG, PG, 1);
		sect(seg("__TEXT_EXEC", 5 * PG, PG, 5), "__text", KV + 5 * PG, PG, S_ATTR_PURE_INSTRUCTIONS);
		symtab(7 * PG + 0x800);
		sym("_fipspost_post_integrity", KV + FIPS);
		for (unsigned i = 0; i < sizeof(words) / sizeof(words[0]); i++) *(u32_t *)(file + words[i].off) = words[i].before;
		for (unsigned i = 0; i < PG; i += 4) if (!*(u32_t *)(file + 5 * PG + i)) *(u32_t *)(file + 5 * PG + i) = 0x91000400 + i;	/* add x0, x0, #... */
		return end > st->stroff + st->strsize ? end : st->stroff + st->strsize;
	}
}

/* the collection parsed and placed as mode 3 places it (macho_load, then macho_copy to where its lowest address is to live) */
static unsigned char *kc_at;
#define KC_SIZE (7 * PG)
static int kc_place(unsigned long n)
{
	if (macho_load(&img, memcpy(malloc(n), file, n), n, ~0UL)) { printf("test: macho_load refused the collection\n"); return 1; }
	kc_at = malloc(KC_SIZE); memset(kc_at, 0xa5, KC_SIZE);	/* what no segment's file bytes cover stays as it was */
	img.slide = (u64_t)kc_at - img.vmlow;
	if (macho_copy(&img, kc_at)) { printf("test: macho_copy refused the collection\n"); return 1; }
	return 0;
}
static int c_kc(void)
{
	static const struct { const char *name; long off; } syms[] = {	/* -1: not a symbol of the kernel's that has an address in the image */
		{ "_arm_init", 4 * PG + 0x100 }, { "_arm_init_tramp", 4 * PG + 0x200 }, { "_arm", 4 * PG + 0x280 }, { "_linux64_hook", 6 * PG },
		{ "_last", 7 * PG - 1 }, { "_outside", -1 }, { "_undefined", -1 }, { "_arm_ini", -1 }, { "", -1 }, { "_fipspost_post_integrity", -1 },
	};
	unsigned long n = kc();

	bad = 0;
	if (kc_place(n)) return 1;
	CK(img.filetype == MH_FILESET && img.vmlow == KV && img.vmhigh == KV + KC_SIZE, "the collection is taken for %#llx to %#llx, type %u", X(img.vmlow),
	    X(img.vmhigh), img.filetype);
	CK(img.kernel_mh == KV + PG, "the kernel's header is taken to be at %#llx: com.apple.kernel's entry is at %#llx", X(img.kernel_mh), X(KV + PG));
	CK(!memcmp(kc_at, file, KC_SIZE), "the placed collection is not its segments' file bytes, each at its address");
	for (unsigned i = 0; i < sizeof(syms) / sizeof(syms[0]); i++) {
		u64_t a = macho_sym(&img, syms[i].name), is = syms[i].off < 0 ? 0 : (u64_t)kc_at + syms[i].off;

		CK(a == is, "macho_sym(\"%s\") is %#llx, not %#llx", syms[i].name, X(a), X(is));
	}
	return bad;
}

/* el0_patch_image on the placed collection: every word is what the file has, but for the ones in `words` and (fips) the function's three */
static int patched(int fips, const char *log)
{
	static unsigned char is[KC_SIZE];
	static const u32_t ret0[3] = { 0xd503245f, 0x52800000, 0xd65f03c0 };	/* bti c; mov w0, #0; ret */

	bad = 0;
	memcpy(is, file, KC_SIZE);
	for (unsigned i = 0; i < sizeof(words) / sizeof(words[0]); i++) *(u32_t *)(is + words[i].off) = words[i].after;
	if (fips) memcpy(is + FIPS, ret0, sizeof(ret0));
	for (unsigned long o = 0; o < KC_SIZE; o += 4)
		if (*(u32_t *)(kc_at + o) != *(u32_t *)(is + o)) {
			CK(0, "the word at %#lx (%#x in the file) is %#x after the patching, not %#x", o, *(u32_t *)(file + o), *(u32_t *)(kc_at + o), *(u32_t *)(is + o));
			break;
		}
	CK(!strncmp(last_log, log, strlen(log)), "the last log line is \"%s\", not \"%s...\"", last_log, log);
	return bad;
}
static int c_patch(void)
{
	unsigned long n = kc();
	unsigned char both[2 * PG], sum[32];
	char hex[65], wrong[66];

	/* the digest it logs: SHA-256 of corecrypto's segments as loaded, before any patch (what el0_fips_sha256 is compared with) */
	memcpy(both, file + 2 * PG, PG); memcpy(both + PG, file + 5 * PG, PG);
	xr_sha256(both, sizeof(both), sum);
	for (int i = 0; i < 32; i++) snprintf(hex + 2 * i, 3, "%02x", sum[i]);
	if (kc_place(n)) return 1;
	el0_patch_image(&img);
	if (patched(0, "xnu: preserving _fipspost_post_integrity (el0_fips_sha256 not set)")) return 1;
	if (strcmp(cc_sha, hex)) { printf("test: the module logged %s for corecrypto, whose SHA-256 is %s\n", cc_sha, hex); return 1; }
	/* given that digest: _fipspost_post_integrity returns 0, and nothing else changes */
	el0_fips_sha256 = hex;
	if (kc_place(n)) return 1;
	el0_patch_image(&img);
	if (patched(1, "xnu: EL0 verified sha256 and patched _fipspost_post_integrity at")) return 1;
	/* given another, or that one with something after it: as with none */
	strcpy(wrong, hex); wrong[63] = wrong[63] == '0' ? '1' : '0'; el0_fips_sha256 = wrong;
	if (kc_place(n)) return 1;
	el0_patch_image(&img);
	if (patched(0, "xnu: preserving _fipspost_post_integrity (el0_fips_sha256 mismatch or invalid hex)")) return 1;
	strcpy(wrong, hex); strcat(wrong, "0");
	if (kc_place(n)) return 1;
	el0_patch_image(&img);
	return patched(0, "xnu: preserving _fipspost_post_integrity (el0_fips_sha256 mismatch or invalid hex)");
}

/* reader.c itself, on what is known of each of its readers */
static int c_reader(void)
{
	static const struct { const char *line, *name, *is; } args[] = {	/* pexpert/gen/bootargs.c, parse_boot_arg_test's own cases */
		{ "-x -a b=3 y=42", "-a", "1" }, { "-x -a b=3 y=42", "b", "3" }, { "-x -a b=2 ba=3 y=42", "b", "2" }, { "-x -a ba=3 b=2 y=42", "b", "2" },
		{ "-x -a b=2 ba=3 y=42", "ba", "3" }, { "-x -ab -aa y=42", "-a", 0 }, { "-x b=96 y=42", "bx", 0 }, { "-x ab=96 y=42", "a", 0 },
		{ "hello=world -foobar abc debug=0xBAADF00D", "notarealthing", 0 }, { "hello=world -foobar abc debug=0xBAADF00D", "hello", "world" },
		{ "hello=world -foobar abc debug=0xBAADF00D", "debug", "0xBAADF00D" }, { "hello=world -foobar abc debug=0xBAADF00D", "-foobar", "1" },
		{ "hello=world -foobar abc debug=0xBAADF00D", "abc", "1" }, { "debug=0x8 -v\tdebug=0x14e", "debug", "0x14e" }, { "", "debug", 0 },
	};
	static const unsigned char abc[32] = { 0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
		0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad };	/* FIPS 180-4's example */
	struct { uint32_t np, nc; char name[32]; uint32_t len; char val[8]; uint32_t np2, nc2; char name2[32]; uint32_t len2; char val2[4]; } tree =
	    { 1, 1, "name", 5, "root", 1, 0, "name", 2, "c" };
	unsigned char sum[32];
	struct xr_node *t;
	char v[64], err[200];

	bad = 0;
	for (unsigned i = 0; i < sizeof(args) / sizeof(args[0]); i++) {
		int found = xr_bootarg(args[i].line, args[i].name, v, sizeof(v));

		CK(args[i].is ? found && !strcmp(v, args[i].is) : !found, "xr_bootarg(\"%s\", \"%s\") is %s", args[i].line, args[i].name, found ? v : "not found");
	}
	xr_sha256("abc", 3, sum);
	CK(!memcmp(sum, abc, 32), "xr_sha256(\"abc\") is not FIPS 180-4's");
	t = xr_dt_parse(&tree, sizeof(tree), err, sizeof(err));
	CK(t && xr_lookup(t, "/c") && xr_str_eq(xr_prop(xr_lookup(t, "/c"), "name"), "c"), "a tree of two nodes: %s", err);
	CK(!xr_dt_parse(&tree, sizeof(tree) - 4, err, sizeof(err)), "a tree whose last value is past its length is read");
	CK(!xr_dt_parse(&tree, sizeof(tree) + 4, err, sizeof(err)), "a tree shorter than its length is read");
	tree.len2 = 0x80000002;	/* the flag a bootloader's tree has in a length's top bit: XNU takes the length whole */
	CK(!xr_dt_parse(&tree, sizeof(tree), err, sizeof(err)), "a length with its top bit set is read as XNU does not read it");
	tree.len2 = 2; tree.np2 = 0;
	CK(!xr_dt_parse(&tree, sizeof(tree), err, sizeof(err)), "a node without properties is read");
	tree.np2 = 1; memset(tree.name2, 'n', 32);
	CK(!xr_dt_parse(&tree, sizeof(tree), err, sizeof(err)), "a name that is not terminated is read");
	return bad;
}

static const struct { const char *what; int (*run)(void); } cases[] = {
	{ "mode 2 with the defaults: a bare kernel loads, its threaded fixups applied", c_defaults },
	{ "mode 2, ram_mb=4: the framebuffer is inside the region", c_ram4 },
	{ "mode 2, ram_mb=8 and a 9 MiB RAM disk: the RAM disk is inside the region", c_ramdisk },
	{ "mode 0 (every mode but 1 and 3 boots as 2 does): the boot data is inside the region", c_mode0 },
	{ "boot data and RAM whose sizes wrap are refused", c_wrap },
	{ "a threaded fixup past the image is refused", c_fixup },
	{ "a __thread_starts section outside the image is refused", c_starts },
	{ "load commands that a segment laid over them changed are not slid", c_overlap },
	{ "a Mach-O header outside the image is refused", c_header },
	{ "EL0 host: the log line for corecrypto patches that are not the expected two", c_fips },
	{ "a log key longer than the line is cut", c_logkv },
	{ "bootargs that do not fit XNU's command line are refused, and said", c_bootargs_long },
	{ "bootargs that fit load as before", c_bootargs },
	{ "the reader the next cases read XNU's boot data with: XNU's own boot-arg cases, SHA-256, device trees it must refuse", c_reader },
	{ "what XNU is handed with the defaults: boot arguments, device tree, framebuffer, its entry and its variables", c_handed },
	{ "the same with the default ram_mb", c_handed_256 },
	{ "a fixup chain with an authenticated pointer and a bind", c_chain },
	{ "no image: refused, the RAM disk and the trust cache given back", c_no_image },
	{ "a kernel without linux64_hook is refused", c_no_hook },
	{ "a kernel built for another linux64_abi.h is refused, and said", c_abi },
	{ "cpus=4 ecpus=0xa: four CPU nodes, the second and the fourth efficiency cores", c_cpus4 },
	{ "cpus=0: one CPU", c_cpus0 },
	{ "cpus=-3 ecpus=1: one CPU, an efficiency core", c_cpus_neg },
	{ "cpus=12 on a host of 4: the host's 4", c_cpus_host },
	{ "CPUs told apart by MPIDR affinity 1: each node's reg is its CPU's", c_cpus_aff1 },
	{ "cpus=64 on a host of 64: the 32 XNU's tables hold", c_cpus_max },
	{ "a host CPU whose MPIDR has another's slot: the CPUs below it", c_cpus_slot },
	{ "model, soc_name and gpu_model: in the root, /product and /sgx", c_names },
	{ "soc_name and gpu_model given empty: no property", c_names_empty },
	{ "model=xnulinux: listed once in the root's compatible", c_model_own },
	{ "a model of 78 characters: the last that fits the root's compatible", c_model_78 },
	{ "a soc_name the device tree has no room for is refused", c_name_huge },
	{ "root_uuid, boot_uuid and preboot_uuid: root-matching, boot-uuid, apfs-preboot-uuid", c_uuids },
	{ "the three given empty: no property", c_uuids_empty },
	{ "a root_uuid of 35 characters is refused, and said", c_root_uuid_short },
	{ "a boot_uuid of 41 characters is refused, and said", c_boot_uuid_long },
	{ "a preboot_uuid that is not hexadecimal is refused, and said", c_preboot_uuid_bad },
	{ "a RAM disk and a trust cache: in XNU's memory, in /chosen/memory-map, rd=md0", c_blobs },
	{ "a RAM disk of whole sectors, not whole 4 KB pages: md0 holds all of it", c_rd_512 },
	{ "a trust cache that ends on a page, no framebuffer", c_tc_only },
	{ "hostdisk: rd=md0 and no RAMDisk", c_hostdisk },
	{ "fb_height=0: no framebuffer, no Video, no /arm-io/gpu", c_fb_none },
	{ "fb_width=-1280: none either", c_fb_neg },
	{ "a 1366 x 767 framebuffer", c_fb_odd },
	{ "vm16k=0: 4 KB VM pages on the command line", c_vm4k },
	{ "a 1 GHz counter: timebase-frequency, and the 24 MHz timebase asked of XNU", c_hz },
	{ "bootargs end the command line, and theirs replace the built-in debug= and serial=", c_args },
	{ "the longest bootargs that fit fill the command line to its last character", c_args_edge },
	{ "an EL0 host: the kernel told so, the EL0 upcall, its trap and ID register read patched", c_el0_handed },
	{ "an EL0 host without pointer authentication is refused", c_el0_nopac },
	{ "ram_mb of 0, below 0 and above 65536 is refused", c_ram_bad },
	{ "ram_mb of 55 GiB: memSize and dram-size say so", c_ram_55g },
	{ "ram_mb of 56 GiB, where XNU's arm_vm_init panics, is refused, and said", c_ram_56g },
	{ "ram_mb 1 MiB below that, over it with the image and the boot data, as well", c_ram_56g_less },
	{ "mode 3's boot data: the image in its window, memory at its own physical address", c_mode3_data },
	{ "mode 3 asks the host for the image, the boot data's room and ram_mb", c_mode3_ask },
	{ "mode 3 on an EL0 host asks for the same", c_mode3_ask_el0 },
	{ "mode 3, ram_mb of 55 GiB: asked of the host", c_mode3_55g },
	{ "mode 3, ram_mb of 56 GiB: refused before the host is asked, and said", c_mode3_56g },
	{ "mode 3 on an EL0 host, ram_mb of 56 GiB: the same", c_mode3_56g_el0 },
	{ "a kernel collection: its kernel entry found, its segments placed, its symbols looked up", c_kc },
	{ "EL0 host: the collection's traps and ID register reads patched and no other word; corecrypto's digest and integrity check", c_patch },
};

int main(void)
{
	unsigned failed = 0;

	for (unsigned i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
		FILE *out = tmpfile();	/* the case's log, shown when it fails */
		int status = 1, c;
		pid_t pid;

		fflush(stdout);
		if (!out || (pid = fork()) < 0) { perror("test"); return 2; }
		if (!pid) {
			dup2(fileno(out), 1); dup2(fileno(out), 2);
			status = cases[i].run();
			fflush(stdout);
			_exit(status);
		}
		waitpid(pid, &status, 0);
		printf("%s %u - %s\n", status ? "not ok" : "ok", i + 1, cases[i].what);
		if (status) {
			failed++;
			for (rewind(out); (c = getc(out)) != EOF;) putchar(c);
		}
		fclose(out);
	}
	if (failed) printf("xnu.ko loader: %u of %u cases FAILED\n", failed, (unsigned)(sizeof(cases) / sizeof(cases[0])));
	else printf("xnu.ko loader: pass\n");
	return failed != 0;
}

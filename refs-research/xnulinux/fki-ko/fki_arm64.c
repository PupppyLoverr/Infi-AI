// SPDX-License-Identifier: GPL-2.0-only
/*
 * Foreign kernel interface, version 1: native arm64 (EL1) backend.
 */
#include "fki.h"
#include "fki_internal.h"
#include <linux/efi.h>
#include <linux/gfp.h>
#include <linux/io.h>
#include <linux/ioport.h>
#include <linux/irqflags.h>
#include <linux/kprobes.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/preempt.h>
#include <linux/printk.h>
#include <linux/sched.h>
#include <linux/smp.h>
#include <linux/uaccess.h>
#include <linux/vmalloc.h>
#include <asm/arch_timer.h>
#include <asm/barrier.h>
#include <asm/cpufeature.h>
#include <asm/neon.h>
#include <asm/simd.h>
#include <asm/stacktrace.h>
#include <asm/sysreg.h>
#include <asm/tlbflush.h>
#include <asm/virt.h>

unsigned long fki_caps(void)
{
	unsigned long caps = FKI_CAP_EXEC_ONLY;

	if (system_supports_address_auth())
		caps |= FKI_CAP_PAC;
	return caps;
}
EXPORT_SYMBOL(fki_caps);

int fki_cpu_id(void)
{
	return smp_processor_id();
}
EXPORT_SYMBOL(fki_cpu_id);

/*
 * Out-of-tree form only: stock kernels do not export set_memory_x(). The in-tree FKI
 * patch calls it directly; here it is found once through kallsyms (via a kprobe).
 */
typedef int (*set_memory_fn)(unsigned long addr, int numpages);
typedef struct page *(*alloc_contig_fn)(unsigned long nr_pages, gfp_t gfp, int nid, nodemask_t *nodemask);
typedef void (*free_contig_fn)(unsigned long pfn, unsigned long nr_pages);
static alloc_contig_fn p_alloc_contig;
static free_contig_fn p_free_contig;
struct cma;
static struct cma **p_cma_area;	/* the default CMA area (cma= on the command line), if any */
static struct page *(*p_cma_alloc)(struct cma *, unsigned long, unsigned int, bool);	/* not exported on every kernel */
static bool (*p_cma_release)(struct cma *, const struct page *, unsigned long);
static set_memory_fn p_set_memory_x, p_set_memory_nx;
static unsigned long p_stext, p_end;
static u64 *p_kimage_voffset;
static int (*p_efi_mem_type)(unsigned long);	/* the firmware's memory map: is the carve-out RAM at all */

static int fki_resolve(void)
{
	static unsigned long (*lookup)(const char *);
	struct kprobe kp = { .symbol_name = "kallsyms_lookup_name" };

	if (p_set_memory_x)
		return 0;
	if (!lookup) {
		if (register_kprobe(&kp))
			return -ENOENT;
		lookup = (void *)kp.addr;
		unregister_kprobe(&kp);
	}
	p_set_memory_x = (set_memory_fn)lookup("set_memory_x");
	p_alloc_contig = (alloc_contig_fn)lookup("alloc_contig_pages_noprof");
	if (!p_alloc_contig)
		p_alloc_contig = (alloc_contig_fn)lookup("alloc_contig_pages");
	p_free_contig = (free_contig_fn)lookup("free_contig_range");
	p_cma_area = (struct cma **)lookup("dma_contiguous_default_area");
	p_cma_alloc = (void *)lookup("cma_alloc");
	p_cma_release = (void *)lookup("cma_release");
	p_set_memory_nx = (set_memory_fn)lookup("set_memory_nx");
	p_stext = lookup("_stext");
	p_end = lookup("_end");
	p_kimage_voffset = (u64 *)lookup("kimage_voffset");
	p_efi_mem_type = (void *)lookup("efi_mem_type");
	return p_set_memory_x && p_set_memory_nx ? 0 : -ENOENT;
}

int fki_region_alloc(unsigned long size, struct fki_region *r)
{
	unsigned long n = PAGE_ALIGN(size) >> PAGE_SHIFT;
	void *va;
	int err;

	/* vmap() memory is never executable on current kernels; vmalloc + set_memory_x is (out-of-tree form). */
	err = fki_resolve();
	if (err)
		return err;
	va = vzalloc(n << PAGE_SHIFT);
	if (!va)
		return -ENOMEM;
	err = p_set_memory_x((unsigned long)va, n);
	if (err) {
		vfree(va);
		return err;
	}
	r->va = va;
	r->pa = page_to_phys(vmalloc_to_page(va));
	r->size = n << PAGE_SHIFT;
	r->handle = NULL;
	return 0;
}
EXPORT_SYMBOL(fki_region_alloc);

void fki_region_free(struct fki_region *r)
{
	p_set_memory_nx((unsigned long)r->va, r->size >> PAGE_SHIFT);
	vfree(r->va);
}
EXPORT_SYMBOL(fki_region_free);

#define FKI_SCTLR_EN (SCTLR_ELx_ENIA | SCTLR_ELx_ENIB | SCTLR_ELx_ENDA | SCTLR_ELx_ENDB)

static __always_inline void keys_read(struct fki_pac_keys *k)
{
	k->ia[0] = read_sysreg_s(SYS_APIAKEYLO_EL1); k->ia[1] = read_sysreg_s(SYS_APIAKEYHI_EL1);
	k->ib[0] = read_sysreg_s(SYS_APIBKEYLO_EL1); k->ib[1] = read_sysreg_s(SYS_APIBKEYHI_EL1);
	k->da[0] = read_sysreg_s(SYS_APDAKEYLO_EL1); k->da[1] = read_sysreg_s(SYS_APDAKEYHI_EL1);
	k->db[0] = read_sysreg_s(SYS_APDBKEYLO_EL1); k->db[1] = read_sysreg_s(SYS_APDBKEYHI_EL1);
	k->ga[0] = read_sysreg_s(SYS_APGAKEYLO_EL1); k->ga[1] = read_sysreg_s(SYS_APGAKEYHI_EL1);
	k->sctlr_en = read_sysreg(sctlr_el1) & FKI_SCTLR_EN;
}

static __always_inline void keys_write(const struct fki_pac_keys *k)
{
	write_sysreg_s(k->ia[0], SYS_APIAKEYLO_EL1); write_sysreg_s(k->ia[1], SYS_APIAKEYHI_EL1);
	write_sysreg_s(k->ib[0], SYS_APIBKEYLO_EL1); write_sysreg_s(k->ib[1], SYS_APIBKEYHI_EL1);
	write_sysreg_s(k->da[0], SYS_APDAKEYLO_EL1); write_sysreg_s(k->da[1], SYS_APDAKEYHI_EL1);
	write_sysreg_s(k->db[0], SYS_APDBKEYLO_EL1); write_sysreg_s(k->db[1], SYS_APDBKEYHI_EL1);
	write_sysreg_s(k->ga[0], SYS_APGAKEYLO_EL1); write_sysreg_s(k->ga[1], SYS_APGAKEYHI_EL1);
	sysreg_clear_set(sctlr_el1, FKI_SCTLR_EN, k->sctlr_en & FKI_SCTLR_EN);
	isb();
}

void fki_fpsimd_forget(void)
{
#ifdef scoped_ksimd	/* 6.19 on: kernel_neon_begin() takes the caller's buffer (the change that brought scoped_ksimd()) */
	struct user_fpsimd_state scratch;	/* where Linux would save a kernel-mode state over a context switch: none here */

	kernel_neon_begin(&scratch);
	kernel_neon_end(&scratch);
#else
	kernel_neon_begin();
	kernel_neon_end();
#endif
}

__attribute__((target("branch-protection=none"))) unsigned long fki_enter(const struct fki_pac_keys *foreign, struct fki_pac_keys *saved)
{
	unsigned long flags;

	fki_fpsimd_forget();
	preempt_disable();
	local_irq_save(flags);
	keys_read(saved);
	keys_write(foreign);
	return flags;
}
EXPORT_SYMBOL(fki_enter);

__attribute__((target("branch-protection=none"))) void fki_leave(unsigned long cookie, const struct fki_pac_keys *saved)
{
	keys_write(saved);
	local_irq_restore(cookie);
	preempt_enable();
}
EXPORT_SYMBOL(fki_leave);

__attribute__((target("branch-protection=none"))) int fki_pac_swap(const struct fki_pac_keys *load, struct fki_pac_keys *save)
{
	keys_read(save);
	keys_write(load);
	return 0;
}
EXPORT_SYMBOL(fki_pac_swap);

fki_u64 fki_counter_frequency(void)
{
	return arch_timer_get_cntfrq();
}
EXPORT_SYMBOL(fki_counter_frequency);

fki_u64 fki_counter(void)
{
	return __arch_counter_get_cntvct();
}
EXPORT_SYMBOL(fki_counter);

/* ---- foreign kernel faults ---- */

static unsigned long fki_fault_lo, fki_fault_hi;
static int (*fki_fault_handler)(unsigned long far, unsigned long esr, void *regs);

static int fki_fault_pre(struct kprobe *p, struct pt_regs *kregs)
{
	unsigned long far = kregs->regs[0], esr = kregs->regs[1];
	struct pt_regs *regs = (struct pt_regs *)kregs->regs[2];
	int (*h)(unsigned long, unsigned long, void *) = READ_ONCE(fki_fault_handler);

	if (!h || user_mode(regs))
		return 0;
	if ((far < fki_fault_lo || far >= fki_fault_hi) &&
	    (instruction_pointer(regs) < fki_fault_lo || instruction_pointer(regs) >= fki_fault_hi))
		return 0;
	if (h(far, esr, regs))
		return 0;
	instruction_pointer_set(kregs, kregs->regs[30]);	/* __do_kernel_fault returns at once */
	return 1;
}
NOKPROBE_SYMBOL(fki_fault_pre);

unsigned long fki_regs_pc(void *regs) { return instruction_pointer((struct pt_regs *)regs); }
EXPORT_SYMBOL(fki_regs_pc);
unsigned long fki_regs_lr(void *regs) { return ((struct pt_regs *)regs)->regs[30]; }
EXPORT_SYMBOL(fki_regs_lr);
unsigned long fki_regs_pstate(void *regs) { return ((struct pt_regs *)regs)->pstate; }
EXPORT_SYMBOL(fki_regs_pstate);
void fki_regs_set_pc(void *regs, unsigned long pc) { instruction_pointer_set((struct pt_regs *)regs, pc); }
EXPORT_SYMBOL(fki_regs_set_pc);
void fki_regs_set_reg(void *regs, unsigned int n, unsigned long v) { if (n < 31) ((struct pt_regs *)regs)->regs[n] = v; }
EXPORT_SYMBOL(fki_regs_set_reg);
unsigned long fki_regs_reg(void *regs, unsigned int n) { return n < 31 ? ((struct pt_regs *)regs)->regs[n] : 0; }
EXPORT_SYMBOL(fki_regs_reg);

static struct kprobe fki_fault_kp = { .symbol_name = "__do_kernel_fault", .pre_handler = fki_fault_pre };
static int fki_fault_registered;

static int fki_die_pre(struct kprobe *p, struct pt_regs *kregs)
{
	struct pt_regs *regs = (struct pt_regs *)kregs->regs[1];
	unsigned long esr = kregs->regs[2];
	int (*h)(unsigned long, unsigned long, void *) = READ_ONCE(fki_fault_handler);

	if (!h || !regs || user_mode(regs))
		return 0;
	if (instruction_pointer(regs) < fki_fault_lo || instruction_pointer(regs) >= fki_fault_hi)
		return 0;
	if (h(0, esr, regs))
		return 0;
	instruction_pointer_set(kregs, kregs->regs[30]);	/* die returns at once */
	return 1;
}
NOKPROBE_SYMBOL(fki_die_pre);
static struct kprobe fki_die_kp = { .symbol_name = "die", .pre_handler = fki_die_pre };
static int fki_die_registered;

int fki_fault_register(unsigned long lo, unsigned long hi, int (*handler)(unsigned long far, unsigned long esr, void *regs))
{
	int err;

	if (fki_fault_registered)
		return -EBUSY;
	fki_fault_lo = lo;
	fki_fault_hi = hi;
	WRITE_ONCE(fki_fault_handler, handler);
	err = register_kprobe(&fki_fault_kp);
	if (err) {
		WRITE_ONCE(fki_fault_handler, NULL);
		return err;
	}
	fki_fault_registered = 1;
	fki_die_registered = register_kprobe(&fki_die_kp) == 0;	/* best effort */
	if (!fki_die_registered)
		pr_warn("fki: no die() probe: foreign non-abort exceptions oops\n");
	return 0;
}
EXPORT_SYMBOL(fki_fault_register);

void fki_fault_unregister(void)
{
	if (!fki_fault_registered)
		return;
	unregister_kprobe(&fki_fault_kp);
	if (fki_die_registered) {
		unregister_kprobe(&fki_die_kp);
		fki_die_registered = 0;
		memset(&fki_die_kp, 0, sizeof(fki_die_kp));
		fki_die_kp.symbol_name = "die";
		fki_die_kp.pre_handler = fki_die_pre;
	}
	WRITE_ONCE(fki_fault_handler, NULL);
	fki_fault_registered = 0;
	memset(&fki_fault_kp, 0, sizeof(fki_fault_kp));
	fki_fault_kp.symbol_name = "__do_kernel_fault";
	fki_fault_kp.pre_handler = fki_fault_pre;
}
EXPORT_SYMBOL(fki_fault_unregister);

fki_u64 fki_phys_peek(fki_u64 pa)
{
	if (!pfn_valid(PHYS_PFN(pa)))
		return ~0ULL;
	return READ_ONCE(*(u64 *)phys_to_virt(pa & ~7ULL));
}
EXPORT_SYMBOL(fki_phys_peek);

/* ---- contiguous memory and the top foreign window ---- */

static unsigned long carve_base, carve_mb, reserve_mb;
module_param(carve_base, ulong, 0444);
module_param(carve_mb, ulong, 0444);
module_param(reserve_mb, ulong, 0444);
static struct page *resv_pg;
static unsigned long resv_cnt;
static bool resv_used, carve_used;

static void fki_reserve(void)
{
	unsigned long cnt = ALIGN(reserve_mb << 20, SZ_2M) >> PAGE_SHIFT;

	if (carve_mb)
		pr_info("fki: %lu MiB at %#lx for the foreign kernel's DRAM (outside the host's memory)\n", carve_mb, carve_base);
	if (!reserve_mb || fki_resolve() || !p_cma_area || !*p_cma_area || !p_cma_alloc)
		return;
	resv_pg = p_cma_alloc(*p_cma_area, cnt, get_order(SZ_2M), true);
	if (resv_pg)
		resv_cnt = cnt;
	pr_info("fki: %lu MiB for the foreign kernel's DRAM %s\n", reserve_mb, resv_pg ? "reserved" : "NOT reserved (CMA)");
}

static int fki_carve_check(unsigned long base, unsigned long len)
{
	unsigned long end = base + len;

	if (end < base) {
		pr_err("fki: carve-out [%#lx, %#lx) overflows physical address space\n", base, end);
		return -EINVAL;
	}
	/* (-ENOTSUPP: there is no firmware map to ask. An address the map does not have is no memory at all.) */
	if (p_efi_mem_type && p_efi_mem_type(base) != -ENOTSUPP) {
		unsigned long a;

		for (a = base; a < end; a += SZ_2M) {
			int t = p_efi_mem_type(a);

			if (t != EFI_LOADER_CODE && t != EFI_LOADER_DATA && t != EFI_BOOT_SERVICES_CODE &&
			    t != EFI_BOOT_SERVICES_DATA && t != EFI_CONVENTIONAL_MEMORY) {
				pr_err("fki: carve-out [%#lx, %#lx) is not all RAM (EFI type %d at %#lx); refusing to map (check carve_base, carve_mb and the VM's memory size)\n",
				       base, end, t, a);
				return -EINVAL;
			}
		}
	}
	if (p_stext && p_end && p_kimage_voffset) {
		unsigned long k_start = p_stext - *p_kimage_voffset;
		unsigned long k_end = p_end - *p_kimage_voffset;

		if (base < k_end && k_start < end) {
			pr_err("fki: carve-out [%#lx, %#lx) overlaps Linux kernel image [%#lx, %#lx); refusing to map (boot with nokaslr or adjust carve_base)\n",
			       base, end, k_start, k_end);
			return -EBUSY;
		}
	}
	if (region_intersects(base, len, IORESOURCE_SYSTEM_RAM, IORES_DESC_NONE) != REGION_DISJOINT) {
		pr_err("fki: carve-out [%#lx, %#lx) overlaps IORESOURCE_SYSTEM_RAM; refusing to map (check mem= and carve_base)\n",
		       base, end);
		return -EBUSY;
	}
	if (region_intersects(base, len, IORESOURCE_MEM, IORES_DESC_NONE) != REGION_DISJOINT) {
		pr_err("fki: carve-out [%#lx, %#lx) overlaps busy/reserved IORESOURCE_MEM; refusing to map (check /proc/iomem and carve_base)\n",
		       base, end);
		return -EBUSY;
	}
	return 0;
}

int fki_contig_alloc(unsigned long size, struct fki_region *r)
{
	unsigned long n = 1;
	struct page *pg;
	int err = fki_resolve();

	if (err)
		return err;
	if (carve_mb && !carve_used && ALIGN(size, SZ_2M) <= carve_mb << 20 && !(carve_base & (SZ_2M - 1))) {
		unsigned long clen = ALIGN(size, SZ_2M);
		void *va;

		err = fki_carve_check(carve_base, clen);
		if (err)
			return err;
		va = memremap(carve_base, clen, MEMREMAP_WB);
		if (va) {
			carve_used = true;
			r->va = va;
			r->size = clen;
			memset(r->va, 0, r->size);
			r->pa = carve_base;
			r->handle = (void *)4UL;	/* bit 2: the carve-out */
			return 0;
		}
		pr_warn("fki: carve-out at %#lx not mappable\n", carve_base);
	}
	if (resv_pg && !resv_used && ALIGN(size, SZ_2M) >> PAGE_SHIFT <= resv_cnt) {
		resv_used = true;
		r->va = page_address(resv_pg);
		r->size = ALIGN(size, SZ_2M);
		memset(r->va, 0, r->size);
		r->pa = page_to_phys(resv_pg);
		r->handle = (void *)((unsigned long)resv_pg | 2);	/* bit 1: the reservation, kept when freed */
		return 0;
	}
	if (size >= (64UL << 20) && p_cma_area && *p_cma_area && p_cma_alloc && p_cma_release) {
		unsigned long cnt = ALIGN(size, SZ_2M) >> PAGE_SHIFT;
		pg = p_cma_alloc(*p_cma_area, cnt, get_order(SZ_2M), true);
		if (pg) {
			r->va = page_address(pg);
			memset(r->va, 0, cnt << PAGE_SHIFT);
			r->pa = page_to_phys(pg);
			r->size = cnt << PAGE_SHIFT;
			r->handle = (void *)((unsigned long)pg | 1);
			return 0;
		}
	}
	if (!p_alloc_contig)
		return -ENOENT;
	while ((n << PAGE_SHIFT) < size)
		n <<= 1;
	pg = p_alloc_contig(n, GFP_KERNEL | __GFP_NOWARN, numa_node_id(), NULL);
	if (!pg)
		return -ENOMEM;
	r->va = page_address(pg);
	memset(r->va, 0, n << PAGE_SHIFT);
	r->pa = page_to_phys(pg);
	r->size = n << PAGE_SHIFT;
	r->handle = pg;
	return 0;
}
EXPORT_SYMBOL(fki_contig_alloc);

void fki_contig_free(struct fki_region *r)
{
	if ((unsigned long)r->handle == 4) {
		memunmap(r->va);
		carve_used = false;
		return;
	}
	if ((unsigned long)r->handle & 2) {
		resv_used = false;	/* back to the reservation, for the next run */
		return;
	}
	if ((unsigned long)r->handle & 1) {
		p_cma_release(*p_cma_area, (struct page *)((unsigned long)r->handle & ~1UL), r->size >> PAGE_SHIFT);
		return;
	}
	if (p_free_contig)
		p_free_contig(page_to_pfn((struct page *)r->handle), r->size >> PAGE_SHIFT);
}
EXPORT_SYMBOL(fki_contig_free);

int fki_window_l1(unsigned long va, unsigned long long **l1_va, fki_u64 *l1_pa)
{
	u64 ttbr1 = read_sysreg(ttbr1_el1) & GENMASK_ULL(47, 1);
	u64 *l0 = phys_to_virt(ttbr1 & ~0xfffULL);
	u64 e = READ_ONCE(l0[(va >> 39) & 511]);

	if ((e & 3) != 3)
		return -ENOENT;
	*l1_pa = e & GENMASK_ULL(47, 12);
	*l1_va = phys_to_virt(*l1_pa);
	return 0;
}
EXPORT_SYMBOL(fki_window_l1);

void fki_window_set(unsigned long long *slot, fki_u64 desc)
{
	WRITE_ONCE(*slot, desc);
	dsb(ishst);
	__tlbi(vmalle1is);
	dsb(ish);
	isb();
}
EXPORT_SYMBOL(fki_window_set);

/* ---- Linux-compliant alternate stack & DAIF preservation ---- */

static void *fki_alt_stack[NR_CPUS];
#define FKI_ALT_STACK_ORDER get_order(2 * THREAD_SIZE)

asm(
"	.pushsection .text\n"
"	.type	fki_run_on_stack, %function\n"
"fki_run_on_stack:\n"
"	hint	#34\n"
"	stp	x29, x30, [sp, #-16]!\n"
"	mov	x29, sp\n"
"	mov	x9, sp\n"
"	mov	sp, x2\n"
"	stp	x9, xzr, [sp, #-16]!\n"
"	mov	x9, x0\n"
"	mov	x0, x1\n"
"	blr	x9\n"
"	ldp	x9, xzr, [sp], #16\n"
"	mov	sp, x9\n"
"	ldp	x29, x30, [sp], #16\n"
"	ret\n"
"	.size	fki_run_on_stack, . - fki_run_on_stack\n"
"	.popsection\n");
void fki_run_on_stack(void (*fn)(void *), void *arg, void *top);

void fki_arch_linux_runs(void (*fn)(void *), void *arg)
{
	unsigned long daif = read_sysreg(daif);
	unsigned long sp = current_stack_pointer;
	int cpu = smp_processor_id();

	if (on_task_stack(current, sp, 1))
		fn(arg);
	else
		fki_run_on_stack(fn, arg, (char *)fki_alt_stack[cpu] + THREAD_SIZE);
	write_sysreg(daif, daif);
}

static void fki_read_mpidr(void *p) { *(u64 *)p = read_cpuid_mpidr(); }

int fki_cpu_mpidr(int cpu, fki_u64 *mpidr)
{
	u64 v = 0;
	int err;

	cpu = fki_cpu_map(cpu);

	if (cpu < 0 || cpu >= nr_cpu_ids || !cpu_online(cpu))
		return -EINVAL;
	err = smp_call_function_single(cpu, fki_read_mpidr, &v, 1);
	*mpidr = v;
	return err;
}
EXPORT_SYMBOL(fki_cpu_mpidr);

/* ---- FKI v1 EL0-host primitives (unsupported on native EL1 arm64) ---- */

int fki_kmem_alloc(unsigned long size, unsigned long high_va, fki_u64 pa_base, struct fki_region *r) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_kmem_alloc);
int fki_kmem_alias(struct fki_region *r, unsigned long va, unsigned long size) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_kmem_alias);
void fki_kmem_free(struct fki_region *r) {}
EXPORT_SYMBOL(fki_kmem_free);
int fki_high_map(unsigned long va, fki_u64 xpa, unsigned long size, int prot) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_high_map);
void fki_high_unmap(unsigned long va, unsigned long size) {}
EXPORT_SYMBOL(fki_high_unmap);
void fki_high_flush(void) {}
EXPORT_SYMBOL(fki_high_flush);
int fki_high_txn_begin(unsigned long va, unsigned long size, void **handle_out)
{
	if (handle_out)
		*handle_out = NULL;
	return -EOPNOTSUPP;
}
EXPORT_SYMBOL(fki_high_txn_begin);
void fki_high_txn_restore(void *txn) {}
EXPORT_SYMBOL(fki_high_txn_restore);
void fki_high_txn_end(void *txn) {}
EXPORT_SYMBOL(fki_high_txn_end);
int fki_high_read_u64x2(unsigned long va, fki_u64 *out0, fki_u64 *out1)
{
	fki_u64 pair[2];

	if (!out0 || !out1 || (va & 7UL) || va < 0xffffff8000000000UL || va > 0xfffffffc00000000UL - sizeof(pair))
		return -EFAULT;
	if (copy_from_kernel_nofault(pair, (const void *)va, sizeof(pair)) != 0)
		return -EFAULT;
	*out0 = pair[0];
	*out1 = pair[1];
	return 0;
}
EXPORT_SYMBOL(fki_high_read_u64x2);
void fki_icache_flush(unsigned long va, unsigned long size)
{
	(void)va;
	(void)size;
	asm volatile("dsb ish; ic ialluis; dsb ish; isb" ::: "memory");
}
EXPORT_SYMBOL(fki_icache_flush);

void *fki_as_create(void) { return NULL; }
EXPORT_SYMBOL(fki_as_create);
void fki_as_destroy(void *as) {}
EXPORT_SYMBOL(fki_as_destroy);
int fki_as_map(void *as, unsigned long va, fki_u64 xpa, unsigned long size, int prot) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_as_map);
void fki_as_unmap(void *as, unsigned long va, unsigned long size) {}
EXPORT_SYMBOL(fki_as_unmap);
void fki_as_flush(void *as) {}
EXPORT_SYMBOL(fki_as_flush);

void *fki_utcb_create(void *as) { return NULL; }
EXPORT_SYMBOL(fki_utcb_create);
void fki_utcb_destroy(void *utcb) {}
EXPORT_SYMBOL(fki_utcb_destroy);
int fki_utcb_set_as(void *utcb, void *as) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_utcb_set_as);
int fki_utcb_set_pac(void *utcb, const struct fki_pac_keys *keys) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_utcb_set_pac);
int fki_utcb_run(void *utcb, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_utcb_run);
int fki_utcb_resume(void *utcb, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop) { return -ENOSYS; }
EXPORT_SYMBOL(fki_utcb_resume);
int fki_utcb_sync_regs(void *utcb, struct fki_utcb_regs *regs) { return -ENOSYS; }
EXPORT_SYMBOL(fki_utcb_sync_regs);
int fki_utcb_fpu_read(void *utcb, void *buf, unsigned long len) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_utcb_fpu_read);
int fki_utcb_fpu_write(void *utcb, const void *buf, unsigned long len) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_utcb_fpu_write);

/* ---- arch init / exit ---- */

static void fki_free_alt_stacks(void)
{
	int cpu;

	for_each_possible_cpu(cpu) {
		if (fki_alt_stack[cpu])
			free_pages((unsigned long)fki_alt_stack[cpu], FKI_ALT_STACK_ORDER);
		fki_alt_stack[cpu] = NULL;
	}
}

int fki_arch_init(void)
{
	int cpu;

	/*
	 * Two hosts are refused here, before a foreign kernel can be started on them (docs/hosted-service.md, 2.1):
	 * - Linux at EL2, a bare-metal kernel with VHE. The foreign kernel is entered at Linux's own exception level and is
	 *   built for EL1. kvm-arm.mode=nvhe on the command line keeps Linux at EL1; a guest is always there.
	 * - no pointer authentication. fki_enter() switches the keys whatever fki_caps() said: without FEAT_PAuth (or with
	 *   it hidden from this kernel) those registers are an undefined instruction, taken in the kernel with interrupts
	 *   off on the first entry.
	 */
	if (is_kernel_in_hyp_mode()) {
		pr_err("fki: Linux runs at EL2 (VHE) here and a foreign kernel needs it at EL1: boot with kvm-arm.mode=nvhe, or use a guest\n");
		return -ENODEV;
	}
	if (!system_supports_address_auth()) {
		pr_err("fki: no pointer authentication (FEAT_PAuth) on this CPU, or this kernel does not use it: the foreign kernel's entry switches its keys\n");
		return -ENODEV;
	}
	for_each_possible_cpu(cpu) {
		struct page *pg = alloc_pages(GFP_KERNEL, FKI_ALT_STACK_ORDER);

		if (!pg) {
			fki_free_alt_stacks();
			return -ENOMEM;
		}
		fki_alt_stack[cpu] = page_address(pg);
	}
	fki_reserve();
	return 0;
}

void fki_arch_exit(void)
{
	if (resv_pg && !resv_used)
		p_cma_release(*p_cma_area, resv_pg, resv_cnt);
	fki_free_alt_stacks();
}

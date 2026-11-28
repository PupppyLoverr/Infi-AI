// SPDX-License-Identifier: GPL-2.0-only
/*
 * Foreign kernel interface, version 1: L4H AArch64 (EL0) backend.
 */
#include "fki.h"
#include "fki_internal.h"
#include <linux/atomic.h>
#include <linux/completion.h>
#include <linux/errno.h>
#include <linux/gfp.h>
#include <linux/io.h>
#include <linux/irqflags.h>
#include <linux/list.h>
#include <linux/minmax.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/overflow.h>
#include <linux/percpu-refcount.h>
#include <linux/preempt.h>
#include <linux/printk.h>
#include <linux/rcupdate.h>
#include <linux/refcount.h>
#include <linux/sched.h>
#include <linux/slab.h>
#include <linux/smp.h>
#include <linux/spinlock.h>
#include <linux/string.h>
#include <linux/xarray.h>
#include <asm/neon.h>
#include <l4h_sys.h>
#include <l4h_caps.h>
#include <l4h_vspace.h>
#include <l4h/provider-registers.h>

#define FKI_L4H_ERR_ILLEGAL_OP	3
#define FKI_L4H_REGION_VA	0xffffffe000000000ULL
#define FKI_L4H_REGION_PA	0x800000000ULL

/* SCTLR_EL1 PAC enable bit positions (ABI of struct fki_pac_keys.sctlr_en). */
#define FKI_SCTLR_ENIA		(1ULL << 31)
#define FKI_SCTLR_ENIB		(1ULL << 30)
#define FKI_SCTLR_ENDA		(1ULL << 27)
#define FKI_SCTLR_ENDB		(1ULL << 13)

static DEFINE_PER_CPU(struct fki_pac_keys, l4h_cur_pac);
static DEFINE_PER_CPU(struct fki_pac_keys, l4h_hw_pac);
static DEFINE_PER_CPU(bool, l4h_hw_pac_valid);
static DEFINE_PER_CPU(struct l4h_uctx *, fki_cur_utcb);

static void fki_utcb_release_sc(void)
{
	struct l4h_uctx *u = this_cpu_read(fki_cur_utcb);

	if (u) {
		if (u->sc_vcpu >= 0)
			l4h_user_deschedule(u);
		this_cpu_write(fki_cur_utcb, NULL);
	}
}

unsigned long fki_caps(void)
{
	unsigned long features = l4h_provider_features();
	unsigned long caps = FKI_CAP_EL0;

	if ((features & L4H_PROVIDER_ARM_HIGH_VSPACE) &&
	    (l4h_bootinfo->flags & L4H_BOOT_HIGH_VSPACE))
		caps |= FKI_CAP_HIGH_WINDOW;
	if (features & L4H_PROVIDER_ARM_PAC)
		caps |= FKI_CAP_PAC;
	return caps;
}
EXPORT_SYMBOL(fki_caps);

int fki_cpu_id(void)
{
	return smp_processor_id();
}
EXPORT_SYMBOL(fki_cpu_id);

int fki_cpu_mpidr(int cpu, fki_u64 *mpidr)
{
	cpu = fki_cpu_map(cpu);
	if (cpu < 0 || cpu >= nr_cpu_ids || !cpu_online(cpu))
		return -EINVAL;
	/* Encodes Aff1:Aff0 so LINUX64_CPU_SLOT(mpidr) == cpu. */
	*mpidr = ((u64)(cpu & 0xf)) | (((u64)((cpu >> 4) & 0xf)) << 8);
	return 0;
}
EXPORT_SYMBOL(fki_cpu_mpidr);

fki_u64 fki_counter_frequency(void)
{
	u64 v;

	asm volatile("mrs %0, cntfrq_el0" : "=r"(v));
	return v;
}
EXPORT_SYMBOL(fki_counter_frequency);

fki_u64 fki_counter(void)
{
	u64 v;

	asm volatile("isb\n\tmrs %0, cntvct_el0" : "=r"(v));
	return v;
}
EXPORT_SYMBOL(fki_counter);

void fki_fpsimd_forget(void)
{
	if (l4h_neon_usable()) {
		kernel_neon_begin();
		kernel_neon_end();
	}
}

static unsigned long sctlr_to_l4h_pac_en(fki_u64 sctlr_en)
{
	unsigned long en = 0;

	if (sctlr_en & FKI_SCTLR_ENIA)
		en |= L4H_PAC_EN_IA;
	if (sctlr_en & FKI_SCTLR_ENIB)
		en |= L4H_PAC_EN_IB;
	if (sctlr_en & FKI_SCTLR_ENDA)
		en |= L4H_PAC_EN_DA;
	if (sctlr_en & FKI_SCTLR_ENDB)
		en |= L4H_PAC_EN_DB;
	return en;
}

static int keys_apply(const struct fki_pac_keys *k, bool force_disable)
{
	unsigned long words[L4H_PAC_KEY_WORDS];
	unsigned long en;
	int err;

	en = sctlr_to_l4h_pac_en(k->sctlr_en);
	if (!(l4h_provider_features() & L4H_PROVIDER_ARM_PAC)) {
		if (en)
			return -EOPNOTSUPP;
		*this_cpu_ptr(&l4h_cur_pac) = *k;
		return 0;
	}
	if (!en && !force_disable && !IS_ENABLED(CONFIG_ARM64_PTR_AUTH_KERNEL)) {
		*this_cpu_ptr(&l4h_cur_pac) = *k;
		return 0;
	}
	if (*this_cpu_ptr(&l4h_hw_pac_valid) &&
	    memcmp(this_cpu_ptr(&l4h_hw_pac), k, sizeof(*k)) == 0) {
		*this_cpu_ptr(&l4h_cur_pac) = *k;
		return 0;
	}
	if (en) {
		words[0] = k->ia[0]; words[1] = k->ia[1];
		words[2] = k->ib[0]; words[3] = k->ib[1];
		words[4] = k->da[0]; words[5] = k->da[1];
		words[6] = k->db[0]; words[7] = k->db[1];
		words[8] = k->ga[0]; words[9] = k->ga[1];
	}
	err = l4h_tcb_set_pac(0, en, en ? words : NULL);
	if (err == FKI_L4H_ERR_ILLEGAL_OP) {
		pr_err_ratelimited("fki: l4h_tcb_set_pac illegal op despite L4H_PROVIDER_ARM_PAC\n");
		return -EOPNOTSUPP;
	}
	if (err) {
		pr_err_ratelimited("fki: l4h_tcb_set_pac failed: %d\n", err);
		return -EIO;
	}
	*this_cpu_ptr(&l4h_hw_pac) = *k;
	*this_cpu_ptr(&l4h_hw_pac_valid) = true;
	*this_cpu_ptr(&l4h_cur_pac) = *k;
	return 0;
}

unsigned long fki_enter(const struct fki_pac_keys *foreign, struct fki_pac_keys *saved)
{
	unsigned long flags;
	int err;

	fki_fpsimd_forget();
	preempt_disable();
	local_irq_save(flags);
	*saved = *this_cpu_ptr(&l4h_cur_pac);
	err = keys_apply(foreign, false);
	BUG_ON(err);
	return flags;
}
EXPORT_SYMBOL(fki_enter);

void fki_leave(unsigned long cookie, const struct fki_pac_keys *saved)
{
	int err;

	fki_utcb_release_sc();
	err = keys_apply(saved, true);
	BUG_ON(err);
	local_irq_restore(cookie);
	preempt_enable();
}
EXPORT_SYMBOL(fki_leave);

int fki_pac_swap(const struct fki_pac_keys *load, struct fki_pac_keys *save)
{
	*save = *this_cpu_ptr(&l4h_cur_pac);
	return keys_apply(load, false);
}
EXPORT_SYMBOL(fki_pac_swap);

/* ---- alternate stack for Linux calls from hosted kernel stacks ---- */

static void *fki_alt_stack[NR_CPUS];
#define FKI_ALT_STACK_ORDER get_order(2 * THREAD_SIZE)

asm(
"	.pushsection .text\n"
"	.type	fki_run_on_stack, %function\n"
"fki_run_on_stack:\n"
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
	unsigned long sp;
	int cpu = smp_processor_id();

	fki_utcb_release_sc();
	asm volatile("mov %0, sp" : "=r"(sp));
	if (sp >= 0xffffffe000000000ULL && fki_alt_stack[cpu])
		fki_run_on_stack(fn, arg, (char *)fki_alt_stack[cpu] + THREAD_SIZE);
	else
		fn(arg);
}

/* ---- high-window kernel memory & dynamic 4 KiB mappings ---- */

struct fki_pt_node {
	struct list_head list;
	struct l4h_obj pt;
};

struct fki_kmem_chunk {
	bool is_large;
	struct page *large_pg;
	unsigned long large_gpa;
	unsigned long large_slot;
	struct page **small_pgs;	/* 512 entries when !is_large */
	unsigned long *small_slots;	/* 512 entries when !is_large */
	unsigned long alias_large_slot;		/* the chunk's second mapping (fki_kmem_alias), when is_large */
	unsigned long *alias_small_slots;	/* the same, 512 entries when !is_large */
};

#define FKI_AS_SLOT_BITS	30
#define FKI_AS_SLOT_MASK	((1UL << FKI_AS_SLOT_BITS) - 1)
#define FKI_AS_PROT_SHIFT	FKI_AS_SLOT_BITS
#define FKI_AS_PROT_MASK	7UL
#define FKI_AS_PFN_SHIFT	(FKI_AS_SLOT_BITS + 3)

static inline unsigned long fki_eff_prot(int lprot)
{
	lprot &= FKI_AS_PROT_MASK;
	return (lprot & L4H_PROT_WRITE) ? (lprot | L4H_PROT_READ) : lprot;
}

static inline void *fki_as_mk_entry(unsigned long cptr, unsigned long pfn, unsigned long lprot)
{
	return xa_mk_value((pfn << FKI_AS_PFN_SHIFT) |
			   ((lprot & FKI_AS_PROT_MASK) << FKI_AS_PROT_SHIFT) |
			   (cptr & FKI_AS_SLOT_MASK));
}

static inline unsigned long fki_as_entry_cptr(void *e)
{
	return xa_to_value(e) & FKI_AS_SLOT_MASK;
}

static inline unsigned long fki_as_entry_pfn(void *e)
{
	return xa_to_value(e) >> FKI_AS_PFN_SHIFT;
}

static inline unsigned long fki_as_entry_prot(void *e)
{
	return (xa_to_value(e) >> FKI_AS_PROT_SHIFT) & FKI_AS_PROT_MASK;
}

struct fki_high_txn_page {
	unsigned long cptr;
	unsigned long pfn;
	u8 prot;
	bool stashed;
};

struct fki_high_txn {
	struct list_head list;
	struct fki_kmem *km;
	unsigned long va;
	unsigned long size;
	unsigned long npages;
	bool restoring;
	struct fki_high_txn_page pages[];
};

struct fki_kmem {
	unsigned long high_va;
	fki_u64 pa_base;
	unsigned long size;
	unsigned long alias_va;		/* fki_kmem_alias: the first alias_size bytes are mapped here as well */
	unsigned long alias_size;
	unsigned long nchunks;
	struct fki_kmem_chunk *chunks;
	struct list_head pts;
	struct list_head as_list;	/* protected by fki_vs_lock */
	struct list_head high_txns;	/* protected by km->lock */
	struct xarray dyn_slots;	/* (va >> PAGE_SHIFT) -> fki_as_mk_entry(cptr, pfn, lprot) */
	struct xarray pt_ready;		/* (va >> 21) -> xa_mk_value(1) when L1..L3 PTs exist */
	spinlock_t lock;
	struct percpu_ref ops_ref;
	struct completion ops_zero;
	bool ops_ref_inited;
	bool dead;
};

static struct fki_kmem __rcu *active_kmem;
static DEFINE_MUTEX(fki_kmem_mutex);

static void fki_kmem_ops_release(struct percpu_ref *ref)
{
	struct fki_kmem *km = container_of(ref, struct fki_kmem, ops_ref);

	complete(&km->ops_zero);
}

static struct fki_kmem *fki_kmem_get(void)
{
	struct fki_kmem *km;

	rcu_read_lock();
	km = rcu_dereference(active_kmem);
	if (!km || READ_ONCE(km->dead) || !percpu_ref_tryget_live(&km->ops_ref))
		km = NULL;
	rcu_read_unlock();
	return km;
}

static void fki_kmem_put(struct fki_kmem *km)
{
	if (km)
		percpu_ref_put(&km->ops_ref);
}

static int fki_vs_pool_init(void);
static void fki_vs_pool_fini(void);

struct fki_bind_ctx {
	unsigned long vs;
	int errs[NR_CPUS];
};

static void bind_high_vspace_cpu(void *arg)
{
	struct fki_bind_ctx *ctx = arg;
	int cpu = smp_processor_id();
	int err = l4h_tcb_set_high_vspace(0, ctx->vs);

	if (cpu < NR_CPUS)
		ctx->errs[cpu] = err;
	if (err)
		pr_err("fki: cpu %d: l4h_tcb_set_high_vspace(%#lx) failed: %d\n",
		       cpu, ctx->vs, err);
}

static int map_high_frame(struct fki_kmem *km, unsigned long frame_slot,
			  unsigned long va, int prot)
{
	unsigned long high_vs = L4H_CPTR(L4H_SLOT_HIGH_VSPACE);
	int err;

	while ((err = l4h_page_map(frame_slot, high_vs, va, prot)) == L4H_ERR_FAILED_LOOKUP) {
		struct fki_pt_node *node = kzalloc(sizeof(*node), GFP_ATOMIC);

		if (!node)
			return -ENOMEM;
		if (l4h_obj_alloc(L4H_OBJ_PT, 0, &node->pt)) {
			kfree(node);
			return -ENOMEM;
		}
		err = l4h_pt_map(L4H_OBJ_PT, node->pt.cptr, high_vs, va);
		if (err) {
			l4h_obj_free(&node->pt);
			kfree(node);
			return -EIO;
		}
		list_add(&node->list, &km->pts);
	}
	if (err)
		return -EIO;
	/* Leaf 4 KiB mapping succeeded: leaf PT for this 2 MiB block is proven present. */
	if (!(prot & L4H_PROT_LARGE))
		(void)xa_store(&km->pt_ready, va >> 21, xa_mk_value(1), GFP_ATOMIC);
	return 0;
}

static int xpa_to_gpa(struct fki_kmem *km, fki_u64 xpa, unsigned long *gpa_out)
{
	unsigned long off, ci, coff;
	fki_u64 pa_end;
	struct fki_kmem_chunk *ch;

	if (!km || check_add_overflow(km->pa_base, (fki_u64)km->size, &pa_end))
		return -EINVAL;
	if (xpa < km->pa_base || xpa >= pa_end)
		return -EINVAL;
	off = (unsigned long)(xpa - km->pa_base);
	ci = off >> 21;
	if (ci >= km->nchunks)
		return -EINVAL;
	coff = off & (SZ_2M - 1);
	ch = &km->chunks[ci];
	if (ch->is_large) {
		*gpa_out = ch->large_gpa + coff;
		return 0;
	}
	*gpa_out = page_to_phys(ch->small_pgs[coff >> PAGE_SHIFT]) + (coff & ~PAGE_MASK);
	return 0;
}

static void kmem_unalias_chunk(struct fki_kmem_chunk *ch)
{
	unsigned long j;

	if (ch->alias_large_slot) {
		l4h_slot_delete(ch->alias_large_slot);
		ch->alias_large_slot = 0;
	}
	if (ch->alias_small_slots) {
		for (j = 0; j < 512; j++) {
			if (ch->alias_small_slots[j])
				l4h_slot_delete(ch->alias_small_slots[j]);
		}
		kfree(ch->alias_small_slots);
		ch->alias_small_slots = NULL;
	}
}

static void kmem_cleanup(struct fki_kmem *km)
{
	struct fki_pt_node *node, *tmp;
	unsigned long idx;
	void *entry;
	unsigned long i, j;

	if (!km)
		return;
	xa_for_each(&km->dyn_slots, idx, entry) {
		unsigned long slot = fki_as_entry_cptr(entry);

		if (slot)
			l4h_slot_delete(slot);
	}
	xa_destroy(&km->dyn_slots);
	xa_destroy(&km->pt_ready);

	if (km->chunks) {
		for (i = 0; i < km->nchunks; i++) {
			struct fki_kmem_chunk *ch = &km->chunks[i];

			kmem_unalias_chunk(ch);
			if (ch->is_large) {
				if (ch->large_slot)
					l4h_slot_delete(ch->large_slot);
				if (ch->large_pg)
					__free_pages(ch->large_pg, 9);
			} else {
				if (ch->small_slots) {
					for (j = 0; j < 512; j++) {
						if (ch->small_slots[j])
							l4h_slot_delete(ch->small_slots[j]);
					}
					kfree(ch->small_slots);
				}
				if (ch->small_pgs) {
					for (j = 0; j < 512; j++) {
						if (ch->small_pgs[j])
							__free_page(ch->small_pgs[j]);
					}
					kfree(ch->small_pgs);
				}
			}
		}
		kvfree(km->chunks);
	}

	list_for_each_entry_safe(node, tmp, &km->pts, list) {
		list_del(&node->list);
		l4h_obj_free(&node->pt);
		kfree(node);
	}
	kfree(km);
}

int fki_kmem_alloc(unsigned long size, unsigned long high_va, fki_u64 pa_base, struct fki_region *r)
{
	unsigned long aligned, nchunks, va_end;
	fki_u64 pa_end;
	struct fki_kmem *km;
	struct fki_bind_ctx *bctx = NULL;
	unsigned long i, j;
	int cpu, err = 0;

	if (!(fki_caps() & FKI_CAP_HIGH_WINDOW))
		return -EOPNOTSUPP;
	if (!size || !r)
		return -EINVAL;
	if (check_add_overflow(size, (unsigned long)(SZ_2M - 1), &aligned))
		return -EINVAL;
	aligned &= ~(unsigned long)(SZ_2M - 1);
	nchunks = aligned >> 21;
	if ((high_va & (SZ_2M - 1)) || (pa_base & (SZ_2M - 1)) || !aligned || !nchunks)
		return -EINVAL;
	if (check_add_overflow(high_va, aligned, &va_end) ||
	    check_add_overflow(pa_base, (fki_u64)aligned, &pa_end))
		return -EINVAL;

	mutex_lock(&fki_kmem_mutex);
	if (rcu_access_pointer(active_kmem)) {
		mutex_unlock(&fki_kmem_mutex);
		return -EBUSY;
	}

	km = kzalloc(sizeof(*km), GFP_KERNEL);
	if (!km) {
		mutex_unlock(&fki_kmem_mutex);
		return -ENOMEM;
	}
	km->high_va = high_va;
	km->pa_base = pa_base;
	km->size = aligned;
	km->nchunks = nchunks;
	INIT_LIST_HEAD(&km->pts);
	INIT_LIST_HEAD(&km->as_list);
	INIT_LIST_HEAD(&km->high_txns);
	xa_init_flags(&km->dyn_slots, XA_FLAGS_LOCK_IRQ);
	xa_init_flags(&km->pt_ready, XA_FLAGS_LOCK_IRQ);
	spin_lock_init(&km->lock);
	init_completion(&km->ops_zero);
	err = percpu_ref_init(&km->ops_ref, fki_kmem_ops_release, 0, GFP_KERNEL);
	if (err) {
		kfree(km);
		mutex_unlock(&fki_kmem_mutex);
		return err;
	}
	km->ops_ref_inited = true;
	km->dead = false;

	km->chunks = kvcalloc(nchunks, sizeof(*km->chunks), GFP_KERNEL);
	if (!km->chunks) {
		percpu_ref_exit(&km->ops_ref);
		kfree(km);
		mutex_unlock(&fki_kmem_mutex);
		return -ENOMEM;
	}

	for (i = 0; i < nchunks; i++) {
		struct fki_kmem_chunk *ch = &km->chunks[i];
		unsigned long cva = high_va + (i << 21);
		struct page *pg = alloc_pages(GFP_KERNEL | __GFP_ZERO | __GFP_NOWARN, 9);
		unsigned long lf = 0;

		if (pg) {
			unsigned long gpa = page_to_phys(pg);

			lf = l4h_large_frame(gpa);
			if (lf) {
				unsigned long slot = l4h_cap_copy(lf, L4H_RIGHTS_ALL);

				if (!slot) {
					__free_pages(pg, 9);
					err = -ENOMEM;
					goto fail;
				}
				err = map_high_frame(km, slot, cva,
						     L4H_PROT_READ | L4H_PROT_WRITE |
						     L4H_PROT_EXEC | L4H_PROT_LARGE);
				if (err) {
					l4h_slot_delete(slot);
					__free_pages(pg, 9);
					goto fail;
				}
				ch->is_large = true;
				ch->large_pg = pg;
				ch->large_gpa = gpa;
				ch->large_slot = slot;
				continue;
			}
			__free_pages(pg, 9);
		}

		/* Fallback: 512 x 4 KiB pages. */
		ch->small_pgs = kcalloc(512, sizeof(*ch->small_pgs), GFP_KERNEL);
		ch->small_slots = kcalloc(512, sizeof(*ch->small_slots), GFP_KERNEL);
		if (!ch->small_pgs || !ch->small_slots) {
			err = -ENOMEM;
			goto fail;
		}
		for (j = 0; j < 512; j++) {
			unsigned long gpa, slot;

			ch->small_pgs[j] = alloc_page(GFP_KERNEL | __GFP_ZERO);
			if (!ch->small_pgs[j]) {
				err = -ENOMEM;
				goto fail;
			}
			gpa = page_to_phys(ch->small_pgs[j]);
			slot = l4h_cap_copy(L4H_FRAME_CPTR(gpa), L4H_RIGHTS_ALL);
			if (!slot) {
				err = -ENOMEM;
				goto fail;
			}
			err = map_high_frame(km, slot, cva + (j << PAGE_SHIFT),
					     L4H_PROT_READ | L4H_PROT_WRITE | L4H_PROT_EXEC);
			if (err) {
				l4h_slot_delete(slot);
				goto fail;
			}
			ch->small_slots[j] = slot;
		}
	}

	err = fki_vs_pool_init();
	if (err)
		goto fail;

	bctx = kzalloc(sizeof(*bctx), GFP_KERNEL);
	if (!bctx) {
		err = -ENOMEM;
		goto fail;
	}
	bctx->vs = L4H_CPTR(L4H_SLOT_HIGH_VSPACE);
	on_each_cpu(bind_high_vspace_cpu, bctx, 1);
	for_each_online_cpu(cpu) {
		if (cpu < NR_CPUS && bctx->errs[cpu]) {
			err = -EIO;
			break;
		}
	}
	if (err) {
		bctx->vs = 0;
		on_each_cpu(bind_high_vspace_cpu, bctx, 1);
		kfree(bctx);
		goto fail;
	}
	kfree(bctx);

	rcu_assign_pointer(active_kmem, km);
	mutex_unlock(&fki_kmem_mutex);

	r->va = (void *)high_va;
	r->pa = pa_base;
	r->size = aligned;
	r->handle = km;
	return 0;

fail:
	fki_vs_pool_fini();
	if (km->ops_ref_inited) {
		percpu_ref_exit(&km->ops_ref);
		km->ops_ref_inited = false;
	}
	kmem_cleanup(km);
	mutex_unlock(&fki_kmem_mutex);
	return err;
}
EXPORT_SYMBOL(fki_kmem_alloc);

/* The second mapping of one 2 MiB chunk, at cva: a large frame where the first is one. */
static int kmem_alias_chunk(struct fki_kmem *km, struct fki_kmem_chunk *ch, unsigned long cva)
{
	const int prot = L4H_PROT_READ | L4H_PROT_WRITE | L4H_PROT_EXEC;
	unsigned long flags, j;
	int err = 0;

	if (ch->is_large) {
		unsigned long lf = l4h_large_frame(ch->large_gpa);
		unsigned long slot = lf ? l4h_cap_copy(lf, L4H_RIGHTS_ALL) : 0;

		if (!slot)
			return -ENOMEM;
		spin_lock_irqsave(&km->lock, flags);
		err = map_high_frame(km, slot, cva, prot | L4H_PROT_LARGE);
		spin_unlock_irqrestore(&km->lock, flags);
		if (err) {
			l4h_slot_delete(slot);
			return err;
		}
		ch->alias_large_slot = slot;
		return 0;
	}
	ch->alias_small_slots = kcalloc(512, sizeof(*ch->alias_small_slots), GFP_KERNEL);
	if (!ch->alias_small_slots)
		return -ENOMEM;
	for (j = 0; j < 512 && !err; j++) {
		unsigned long slot = l4h_cap_copy(L4H_FRAME_CPTR(page_to_phys(ch->small_pgs[j])),
						  L4H_RIGHTS_ALL);

		if (!slot) {
			err = -ENOMEM;
			break;
		}
		spin_lock_irqsave(&km->lock, flags);
		err = map_high_frame(km, slot, cva + (j << PAGE_SHIFT), prot);
		spin_unlock_irqrestore(&km->lock, flags);
		if (err)
			l4h_slot_delete(slot);
		else
			ch->alias_small_slots[j] = slot;
	}
	if (err)
		kmem_unalias_chunk(ch);
	return err;
}

int fki_kmem_alias(struct fki_region *r, unsigned long va, unsigned long size)
{
	struct fki_kmem *km;
	unsigned long va_end, km_va_end, old_n, n, i, flags;
	int err = 0;

	if (!r || ((va | size) & (SZ_2M - 1)) || check_add_overflow(va, size, &va_end))
		return -EINVAL;
	mutex_lock(&fki_kmem_mutex);
	km = r->handle;
	if (!km || rcu_access_pointer(active_kmem) != km) {
		mutex_unlock(&fki_kmem_mutex);
		return -ENODEV;
	}
	n = size >> 21;
	old_n = km->alias_size >> 21;
	km_va_end = km->high_va + km->size;	/* fki_kmem_alloc checked the sum */
	if (n > km->nchunks || (va < km_va_end && va_end > km->high_va) ||
	    (old_n && va != km->alias_va)) {
		mutex_unlock(&fki_kmem_mutex);
		return -EINVAL;
	}
	for (i = old_n; i < n && !err; i++)
		err = kmem_alias_chunk(km, &km->chunks[i], va + (i << 21));
	if (err) {
		/* back to what it was: i - 1 is the chunk that failed, and it has undone itself */
		n = old_n;
		i--;
	} else {
		i = old_n;
	}
	spin_lock_irqsave(&km->lock, flags);
	km->alias_va = va;
	km->alias_size = n << 21;
	spin_unlock_irqrestore(&km->lock, flags);
	if (i > n) {
		while (i-- > n)
			kmem_unalias_chunk(&km->chunks[i]);
		l4h_vspace_flush(L4H_CPTR(L4H_SLOT_HIGH_VSPACE));
	}
	mutex_unlock(&fki_kmem_mutex);
	return err;
}
EXPORT_SYMBOL(fki_kmem_alias);

static void fki_as_drain_list(struct fki_kmem *km);

void fki_kmem_free(struct fki_region *r)
{
	struct fki_kmem *km;
	struct fki_bind_ctx *bctx;

	if (!r)
		return;
	mutex_lock(&fki_kmem_mutex);
	km = r->handle;
	if (!km) {
		mutex_unlock(&fki_kmem_mutex);
		return;
	}
	WRITE_ONCE(km->dead, true);
	if (rcu_access_pointer(active_kmem) == km)
		RCU_INIT_POINTER(active_kmem, NULL);
	synchronize_rcu();
	percpu_ref_kill(&km->ops_ref);
	wait_for_completion(&km->ops_zero);

	bctx = kzalloc(sizeof(*bctx), GFP_KERNEL);
	if (bctx) {
		bctx->vs = 0;
		on_each_cpu(bind_high_vspace_cpu, bctx, 1);
		kfree(bctx);
	} else {
		struct fki_bind_ctx fallback = { .vs = 0 };
		on_each_cpu(bind_high_vspace_cpu, &fallback, 1);
	}
	fki_vs_pool_fini();
	fki_as_drain_list(km);
	{
		struct fki_high_txn *txn, *tmp;
		unsigned long flags;

		spin_lock_irqsave(&km->lock, flags);
		list_for_each_entry_safe(txn, tmp, &km->high_txns, list) {
			unsigned long idx;

			list_del(&txn->list);
			for (idx = 0; idx < txn->npages; idx++) {
				if (txn->pages[idx].stashed && txn->pages[idx].cptr)
					l4h_slot_delete(txn->pages[idx].cptr);
			}
			kfree(txn);
		}
		spin_unlock_irqrestore(&km->lock, flags);
	}
	if (km->ops_ref_inited) {
		percpu_ref_exit(&km->ops_ref);
		km->ops_ref_inited = false;
	}
	kmem_cleanup(km);
	r->va = NULL;
	r->handle = NULL;
	mutex_unlock(&fki_kmem_mutex);
}
EXPORT_SYMBOL(fki_kmem_free);

static int prot_to_l4h(int prot)
{
	int p = 0;

	if (prot & FKI_PROT_READ)
		p |= L4H_PROT_READ;
	if (prot & FKI_PROT_WRITE)
		p |= L4H_PROT_WRITE;
	if (prot & FKI_PROT_EXEC)
		p |= L4H_PROT_EXEC;
	return p;
}

/* The region's second mapping (fki_kmem_alias), which is as permanent as the aperture. */
static bool high_overlaps_alias(struct fki_kmem *km, unsigned long va, unsigned long va_end)
{
	return km->alias_size && va < km->alias_va + km->alias_size && va_end > km->alias_va;
}

static bool high_in_alias(struct fki_kmem *km, unsigned long va, unsigned long va_end)
{
	return km->alias_size && va >= km->alias_va && va_end <= km->alias_va + km->alias_size;
}

static bool high_overlaps_aperture(struct fki_kmem *km, unsigned long va, unsigned long va_end)
{
	unsigned long km_va_end, ap_start, ap_end;

	if (check_add_overflow(km->high_va, km->size, &km_va_end))
		return true;
	if (high_overlaps_alias(km, va, va_end))
		return true;
	ap_start = round_down(km->high_va, SZ_2M);
	ap_end = round_up(km_va_end, SZ_2M);
	if (ap_end < km_va_end)
		ap_end = ULONG_MAX;
	return va < ap_end && va_end > ap_start;
}

static int ensure_high_pt(struct fki_kmem *km, unsigned long va)
{
	unsigned long high_vs = L4H_CPTR(L4H_SLOT_HIGH_VSPACE);
	unsigned long idx = va >> 21;
	int lvl;

	if (xa_load(&km->pt_ready, idx))
		return 0;
	for (lvl = 0; lvl < 3; lvl++) {
		struct fki_pt_node *node = kzalloc(sizeof(*node), GFP_ATOMIC);
		int err;

		if (!node)
			return -ENOMEM;
		if (l4h_obj_alloc(L4H_OBJ_PT, 0, &node->pt)) {
			kfree(node);
			return -ENOMEM;
		}
		err = l4h_pt_map(L4H_OBJ_PT, node->pt.cptr, high_vs, va & ~(SZ_2M - 1));
		if (err) {
			l4h_obj_free(&node->pt);
			kfree(node);
			if (err == L4H_ERR_DELETE_FIRST)
				break;
			return -EIO;
		}
		list_add(&node->list, &km->pts);
	}
	return 0;
}

static struct fki_high_txn *fki_high_txn_find_locked(struct fki_kmem *km,
						     unsigned long cur_va,
						     unsigned long *idx_out)
{
	struct fki_high_txn *txn;

	list_for_each_entry(txn, &km->high_txns, list) {
		if (cur_va >= txn->va && cur_va < txn->va + txn->size) {
			if (idx_out)
				*idx_out = (cur_va - txn->va) >> PAGE_SHIFT;
			return txn;
		}
	}
	return NULL;
}

static void fki_dyn_slot_clear_locked(struct fki_kmem *km, unsigned long cur_va, bool was_stored)
{
	unsigned long vpn = cur_va >> PAGE_SHIFT;
	unsigned long idx = 0;
	struct fki_high_txn *txn = fki_high_txn_find_locked(km, cur_va, &idx);

	if (txn && txn->pages[idx].stashed) {
		xa_store(&km->dyn_slots, vpn, XA_ZERO_ENTRY, GFP_ATOMIC);
		return;
	}
	if (was_stored)
		xa_erase(&km->dyn_slots, vpn);
	else
		xa_release(&km->dyn_slots, vpn);
}

enum fki_hm_kind {
	FKI_HM_NONE = 0,
	FKI_HM_NEW,
	FKI_HM_REPROT,
	FKI_HM_RESTORE,
};

struct fki_hm_page {
	unsigned long new_pfn;
	unsigned long new_slot;
	unsigned long prev_pfn;
	unsigned long prev_prot;
	struct fki_high_txn *txn;
	unsigned long txn_idx;
	u8 kind;
};

static void fki_high_unmap_km(struct fki_kmem *km, unsigned long va, unsigned long size);

/*
 * A change to the window failed and what it had already changed could not be put back: the window is no longer what
 * the foreign kernel was told. That is the foreign kernel's loss, not Linux's (the window maps only the region's own
 * frames, at addresses only the foreign kernel uses), so the host does not panic: the window is marked dead, every
 * later call on it fails (fki_kmem_get), and the foreign kernel ends its run over the first that it needs.
 * Called with km->lock held.
 */
static void fki_high_dead(struct fki_kmem *km, unsigned long va)
{
	if (!READ_ONCE(km->dead))
		pr_alert("fki: the high window could not be put back at %#lx after a failed change: unusable from here on\n", va);
	WRITE_ONCE(km->dead, true);
}

int fki_high_map(unsigned long va, fki_u64 xpa, unsigned long size, int prot)
{
	struct fki_kmem *km = fki_kmem_get();
	unsigned long high_vs = L4H_CPTR(L4H_SLOT_HIGH_VSPACE);
	unsigned long flags, va_end, km_va_end, npages, i, j, mapped = 0;
	unsigned long last_pt_idx = ~0UL;
	fki_u64 xpa_end;
	unsigned long eff_lprot = fki_eff_prot(prot_to_l4h(prot));
	struct fki_hm_page stack_pgs[16];
	struct fki_hm_page *pgs = stack_pgs;
	int err = 0;

	if (!km)
		return -ENODEV;
	if (!size) {
		fki_kmem_put(km);
		return 0;
	}
	if ((va | xpa | size) & ~PAGE_MASK) {
		fki_kmem_put(km);
		return -EINVAL;
	}
	if (check_add_overflow(va, size, &va_end) ||
	    check_add_overflow(xpa, (fki_u64)size, &xpa_end) ||
	    check_add_overflow(km->high_va, km->size, &km_va_end)) {
		fki_kmem_put(km);
		return -EINVAL;
	}
	/* Initial linear aperture is already mapped RWX (with 2 MiB large frames). */
	if (va >= km->high_va && va_end <= km_va_end &&
	    xpa >= km->pa_base && (va - km->high_va) == (unsigned long)(xpa - km->pa_base)) {
		fki_kmem_put(km);
		return 0;
	}
	/* So is its second mapping (fki_kmem_alias). */
	if (high_in_alias(km, va, va_end) &&
	    xpa >= km->pa_base && (va - km->alias_va) == (unsigned long)(xpa - km->pa_base)) {
		fki_kmem_put(km);
		return 0;
	}
	/* Reject any non-identity or cross-boundary overlap with the permanent 2 MiB aperture. */
	if (high_overlaps_aperture(km, va, va_end)) {
		fki_kmem_put(km);
		return -EBUSY;
	}
	if (!eff_lprot) {
		fki_high_unmap_km(km, va, size);
		fki_kmem_put(km);
		return 0;
	}

	npages = size >> PAGE_SHIFT;
	if (npages > ARRAY_SIZE(stack_pgs)) {
		pgs = kcalloc(npages, sizeof(*pgs), GFP_ATOMIC);
		if (!pgs) {
			fki_kmem_put(km);
			return -ENOMEM;
		}
	} else {
		memset(stack_pgs, 0, npages * sizeof(*stack_pgs));
	}

	/* Phase 1a: validate every target xpa -> gpa before taking km->lock. */
	for (i = 0; i < npages; i++) {
		unsigned long gpa;

		err = xpa_to_gpa(km, xpa + ((fki_u64)i << PAGE_SHIFT), &gpa);
		if (err)
			goto out_free;
		pgs[i].new_pfn = gpa >> PAGE_SHIFT;
	}

	/* Phase 1b: under km->lock, ensure PTs, reserve xarray slots, and pre-allocate frame caps. */
	spin_lock_irqsave(&km->lock, flags);
	for (i = 0; i < npages; i++) {
		unsigned long cur_va = va + (i << PAGE_SHIFT);
		unsigned long pt_idx = cur_va >> 21;
		unsigned long vpn = cur_va >> PAGE_SHIFT;
		void *old;

		old = xa_load(&km->dyn_slots, vpn);
		if (!old) {
			unsigned long txn_idx = 0;
			struct fki_high_txn *txn = fki_high_txn_find_locked(km, cur_va, &txn_idx);

			if (txn && txn->restoring && txn->pages[txn_idx].stashed &&
			    txn->pages[txn_idx].cptr != 0 &&
			    txn->pages[txn_idx].pfn == pgs[i].new_pfn) {
				pgs[i].kind = FKI_HM_RESTORE;
				pgs[i].new_slot = txn->pages[txn_idx].cptr;
				pgs[i].txn = txn;
				pgs[i].txn_idx = txn_idx;
				continue;
			}
			if (pt_idx != last_pt_idx) {
				err = ensure_high_pt(km, cur_va);
				if (err)
					break;
				last_pt_idx = pt_idx;
			}
			err = xa_reserve(&km->dyn_slots, vpn, GFP_ATOMIC);
			if (err)
				break;
			pgs[i].new_slot = l4h_cap_copy(L4H_FRAME_CPTR(pgs[i].new_pfn << PAGE_SHIFT),
						       L4H_RIGHTS_ALL);
			if (!pgs[i].new_slot) {
				fki_dyn_slot_clear_locked(km, cur_va, false);
				err = -ENOMEM;
				break;
			}
			pgs[i].kind = FKI_HM_NEW;
		} else if (fki_as_entry_pfn(old) == pgs[i].new_pfn) {
			/* Same PFN: re-protect in place without deleting or copying caps. */
			pgs[i].prev_pfn = pgs[i].new_pfn;
			pgs[i].prev_prot = fki_as_entry_prot(old);
			pgs[i].kind = FKI_HM_REPROT;
		} else {
			/* Cross-PFN live overwrite prohibited: caller must unmap while quiescent first. */
			err = -EBUSY;
			break;
		}
	}
	if (err) {
		for (j = 0; j < i; j++) {
			unsigned long cur_va = va + (j << PAGE_SHIFT);

			if (pgs[j].kind == FKI_HM_NEW) {
				fki_dyn_slot_clear_locked(km, cur_va, false);
				if (pgs[j].new_slot)
					l4h_slot_delete(pgs[j].new_slot);
			}
		}
		spin_unlock_irqrestore(&km->lock, flags);
		goto out_free;
	}

	/* Phase 2: apply all updates; roll back completely if any mapping fails. */
	for (i = 0; i < npages; i++) {
		unsigned long cur_va = va + (i << PAGE_SHIFT);
		unsigned long vpn = cur_va >> PAGE_SHIFT;

		if (pgs[i].kind == FKI_HM_RESTORE) {
			if (l4h_page_map(pgs[i].new_slot, high_vs, cur_va, (int)eff_lprot) != 0) {
				err = -EIO;
				break;
			}
			if (xa_err(xa_store(&km->dyn_slots, vpn,
					    fki_as_mk_entry(pgs[i].new_slot, pgs[i].new_pfn, eff_lprot),
					    GFP_ATOMIC))) {
				l4h_page_unmap(pgs[i].new_slot);
				err = -ENOMEM;
				break;
			}
			pgs[i].txn->pages[pgs[i].txn_idx].cptr = 0;
			pgs[i].txn->pages[pgs[i].txn_idx].stashed = false;
		} else if (pgs[i].kind == FKI_HM_REPROT) {
			void *old = xa_load(&km->dyn_slots, vpn);
			unsigned long old_cptr = old ? fki_as_entry_cptr(old) : 0;

			if (!old_cptr) {
				err = -EIO;
				break;
			}
			if (pgs[i].prev_prot != eff_lprot) {
				if (l4h_page_map(old_cptr, high_vs, cur_va, (int)eff_lprot) != 0) {
					err = -EIO;
					break;
				}
				/* Slot already exists in dyn_slots: in-place xa_store cannot fail allocation. */
				if (xa_err(xa_store(&km->dyn_slots, vpn,
						    fki_as_mk_entry(old_cptr, pgs[i].new_pfn, eff_lprot),
						    GFP_ATOMIC))) {
					err = -ENOMEM;
					if (l4h_page_map(old_cptr, high_vs, cur_va, (int)pgs[i].prev_prot) != 0) {
						fki_high_dead(km, cur_va);	/* the page keeps the new protection, its record the old */
						err = -EIO;
					}
					break;
				}
			}
		} else {
			/* FKI_HM_NEW */
			err = map_high_frame(km, pgs[i].new_slot, cur_va, (int)eff_lprot);
			if (err)
				break;
			if (xa_err(xa_store(&km->dyn_slots, vpn,
					    fki_as_mk_entry(pgs[i].new_slot, pgs[i].new_pfn, eff_lprot),
					    GFP_ATOMIC))) {
				l4h_slot_delete(pgs[i].new_slot);
				pgs[i].new_slot = 0;
				err = -ENOMEM;
				break;
			}
		}
		mapped++;
	}

	if (err) {
		for (j = 0; j < mapped; j++) {
			unsigned long cur_va = va + (j << PAGE_SHIFT);
			unsigned long vpn = cur_va >> PAGE_SHIFT;

			if (pgs[j].kind == FKI_HM_RESTORE) {
				l4h_page_unmap(pgs[j].new_slot);
				xa_store(&km->dyn_slots, vpn, XA_ZERO_ENTRY, GFP_ATOMIC);
				pgs[j].txn->pages[pgs[j].txn_idx].cptr = pgs[j].new_slot;
				pgs[j].txn->pages[pgs[j].txn_idx].stashed = true;
			} else if (pgs[j].kind == FKI_HM_REPROT) {
				if (pgs[j].prev_prot != eff_lprot) {
					void *cur = xa_load(&km->dyn_slots, vpn);
					unsigned long cptr = cur ? fki_as_entry_cptr(cur) : 0;

					if (!cptr ||
					    l4h_page_map(cptr, high_vs, cur_va, (int)pgs[j].prev_prot) != 0) {
						fki_high_dead(km, cur_va);	/* the page stays as the change left it */
						continue;
					}
					xa_store(&km->dyn_slots, vpn,
						 fki_as_mk_entry(cptr, pgs[j].prev_pfn, pgs[j].prev_prot),
						 GFP_ATOMIC);
				}
			} else {
				/* FKI_HM_NEW */
				fki_dyn_slot_clear_locked(km, cur_va, true);
				l4h_slot_delete(pgs[j].new_slot);
				pgs[j].new_slot = 0;
			}
		}
		for (j = mapped; j < npages; j++) {
			unsigned long cur_va = va + (j << PAGE_SHIFT);

			if (pgs[j].kind == FKI_HM_NEW) {
				fki_dyn_slot_clear_locked(km, cur_va, false);
				if (pgs[j].new_slot)
					l4h_slot_delete(pgs[j].new_slot);
			}
		}
	}
	spin_unlock_irqrestore(&km->lock, flags);

out_free:
	if (pgs != stack_pgs)
		kfree(pgs);
	fki_kmem_put(km);
	if (err)
		pr_err_ratelimited("fki: high_map va=0x%lx xpa=0x%llx size=0x%lx prot=0x%x failed: %d\n",
				   va, xpa, size, prot, err);
	return err;
}
EXPORT_SYMBOL(fki_high_map);

static void fki_high_unmap_km(struct fki_kmem *km, unsigned long va, unsigned long size)
{
	unsigned long flags, va_end, km_va_end, ap_lo, ap_hi;
	unsigned long lo[2], hi[2];
	int i;

	if (!km || !size || ((va | size) & ~PAGE_MASK))
		return;
	if (check_add_overflow(va, size, &va_end) ||
	    check_add_overflow(km->high_va, km->size, &km_va_end))
		return;
	ap_lo = round_down(km->high_va, SZ_2M);
	ap_hi = round_up(km_va_end, SZ_2M);
	if (ap_hi < km_va_end)
		ap_hi = ULONG_MAX;

	lo[0] = va;
	hi[0] = min(va_end, ap_lo);
	lo[1] = max(va, ap_hi);
	hi[1] = va_end;
	if (lo[0] >= hi[0] && lo[1] >= hi[1])
		return;

	spin_lock_irqsave(&km->lock, flags);
	for (i = 0; i < 2; i++) {
		unsigned long cur_va;

		for (cur_va = lo[i]; cur_va < hi[i]; cur_va += PAGE_SIZE) {
			unsigned long vpn = cur_va >> PAGE_SHIFT;
			unsigned long idx = 0;
			struct fki_high_txn *txn;

			if (high_overlaps_alias(km, cur_va, cur_va + PAGE_SIZE))
				continue;
			txn = fki_high_txn_find_locked(km, cur_va, &idx);

			if (txn) {
				if (txn->pages[idx].stashed) {
					void *old = xa_load(&km->dyn_slots, vpn);

					if (old) {
						unsigned long old_slot = fki_as_entry_cptr(old);

						if (old_slot)
							l4h_slot_delete(old_slot);
						xa_store(&km->dyn_slots, vpn, XA_ZERO_ENTRY, GFP_ATOMIC);
					}
				} else {
					void *old = xa_erase(&km->dyn_slots, vpn);

					if (old) {
						unsigned long old_slot = fki_as_entry_cptr(old);

						if (old_slot)
							l4h_slot_delete(old_slot);
					}
				}
			} else {
				void *old = xa_erase(&km->dyn_slots, vpn);

				if (old) {
					unsigned long old_slot = fki_as_entry_cptr(old);

					if (old_slot)
						l4h_slot_delete(old_slot);
				}
			}
		}
	}
	spin_unlock_irqrestore(&km->lock, flags);
}

int fki_high_txn_begin(unsigned long va, unsigned long size, void **handle_out)
{
	struct fki_kmem *km;
	struct fki_high_txn *txn, *cur;
	unsigned long high_vs = L4H_CPTR(L4H_SLOT_HIGH_VSPACE);
	unsigned long va_end, km_va_end, ap_lo, ap_hi, npages, flags, idx, j;

	if (!handle_out)
		return -EINVAL;
	*handle_out = NULL;
	if (!size || ((va | size) & ~PAGE_MASK) ||
	    check_add_overflow(va, size, &va_end))
		return -EINVAL;

	km = fki_kmem_get();
	if (!km)
		return -ENODEV;
	if (READ_ONCE(km->dead)) {
		fki_kmem_put(km);
		return -ENODEV;
	}
	if (check_add_overflow(km->high_va, km->size, &km_va_end)) {
		fki_kmem_put(km);
		return -EINVAL;
	}
	ap_lo = round_down(km->high_va, SZ_2M);
	ap_hi = round_up(km_va_end, SZ_2M);
	if (ap_hi < km_va_end)
		ap_hi = ULONG_MAX;

	npages = size >> PAGE_SHIFT;
	txn = kzalloc(struct_size(txn, pages, npages), GFP_ATOMIC);
	if (!txn) {
		fki_kmem_put(km);
		return -ENOMEM;
	}
	txn->km = km;
	txn->va = va;
	txn->size = size;
	txn->npages = npages;

	spin_lock_irqsave(&km->lock, flags);
	if (READ_ONCE(km->dead)) {
		spin_unlock_irqrestore(&km->lock, flags);
		kfree(txn);
		fki_kmem_put(km);
		return -ENODEV;
	}
	list_for_each_entry(cur, &km->high_txns, list) {
		if (va < cur->va + cur->size && cur->va < va_end) {
			spin_unlock_irqrestore(&km->lock, flags);
			kfree(txn);
			fki_kmem_put(km);
			return -EBUSY;
		}
	}
	for (idx = 0; idx < npages; idx++) {
		unsigned long cur_va = va + (idx << PAGE_SHIFT);
		unsigned long vpn = cur_va >> PAGE_SHIFT;
		void *old;

		if (cur_va >= ap_lo && cur_va < ap_hi)
			continue;
		if (high_overlaps_alias(km, cur_va, cur_va + PAGE_SIZE))
			continue;
		old = xa_load(&km->dyn_slots, vpn);
		if (!old)
			continue;
		{
			unsigned long old_slot = fki_as_entry_cptr(old);

			if (!old_slot || l4h_page_unmap(old_slot) != 0) {
				for (j = 0; j < idx; j++) {
					if (txn->pages[j].stashed) {
						unsigned long prev_va = va + (j << PAGE_SHIFT);
						unsigned long prev_vpn = prev_va >> PAGE_SHIFT;

						/* not mapped again: its slot is recorded below all the same, for kmem_cleanup */
						if (l4h_page_map(txn->pages[j].cptr, high_vs,
								 prev_va, (int)txn->pages[j].prot) != 0)
							fki_high_dead(km, prev_va);
						xa_store(&km->dyn_slots, prev_vpn,
							 fki_as_mk_entry(txn->pages[j].cptr,
									 txn->pages[j].pfn,
									 txn->pages[j].prot),
							 GFP_ATOMIC);
					}
				}
				spin_unlock_irqrestore(&km->lock, flags);
				kfree(txn);
				fki_kmem_put(km);
				return -EIO;
			}
			txn->pages[idx].cptr = old_slot;
			txn->pages[idx].pfn = fki_as_entry_pfn(old);
			txn->pages[idx].prot = (u8)fki_as_entry_prot(old);
			txn->pages[idx].stashed = true;
			xa_store(&km->dyn_slots, vpn, XA_ZERO_ENTRY, GFP_ATOMIC);
		}
	}
	list_add(&txn->list, &km->high_txns);
	spin_unlock_irqrestore(&km->lock, flags);
	fki_kmem_put(km);
	*handle_out = txn;
	return 0;
}
EXPORT_SYMBOL(fki_high_txn_begin);

void fki_high_txn_restore(void *handle)
{
	struct fki_kmem *km;
	struct fki_high_txn *cur;
	unsigned long flags;

	if (!handle)
		return;
	km = fki_kmem_get();
	if (!km)
		return;
	spin_lock_irqsave(&km->lock, flags);
	list_for_each_entry(cur, &km->high_txns, list) {
		if (cur == handle) {
			cur->restoring = true;
			break;
		}
	}
	spin_unlock_irqrestore(&km->lock, flags);
	fki_kmem_put(km);
}
EXPORT_SYMBOL(fki_high_txn_restore);

void fki_high_txn_end(void *handle)
{
	struct fki_kmem *km;
	struct fki_high_txn *cur, *found = NULL;
	unsigned long flags, idx;

	if (!handle)
		return;
	km = fki_kmem_get();
	if (!km)
		return;
	spin_lock_irqsave(&km->lock, flags);
	list_for_each_entry(cur, &km->high_txns, list) {
		if (cur == handle) {
			found = cur;
			list_del(&cur->list);
			for (idx = 0; idx < cur->npages; idx++) {
				if (cur->pages[idx].stashed) {
					unsigned long vpn = (cur->va + (idx << PAGE_SHIFT)) >> PAGE_SHIFT;

					if (cur->pages[idx].cptr)
						l4h_slot_delete(cur->pages[idx].cptr);
					if (xa_load(&km->dyn_slots, vpn) == NULL)
						xa_release(&km->dyn_slots, vpn);
				}
			}
			break;
		}
	}
	spin_unlock_irqrestore(&km->lock, flags);
	if (found)
		kfree(found);
	fki_kmem_put(km);
}
EXPORT_SYMBOL(fki_high_txn_end);

void fki_high_unmap(unsigned long va, unsigned long size)
{
	struct fki_kmem *km = fki_kmem_get();

	if (!km)
		return;
	fki_high_unmap_km(km, va, size);
	fki_kmem_put(km);
}
EXPORT_SYMBOL(fki_high_unmap);

void fki_high_flush(void)
{
	struct fki_kmem *km = fki_kmem_get();

	if (!km)
		return;
	if (fki_caps() & FKI_CAP_HIGH_WINDOW)
		l4h_vspace_flush(L4H_CPTR(L4H_SLOT_HIGH_VSPACE));
	fki_kmem_put(km);
}
EXPORT_SYMBOL(fki_high_flush);

static bool high_range_readable_locked(struct fki_kmem *km, unsigned long va, unsigned long size)
{
	unsigned long end, km_end, page_va, last_page;

	if (!size || check_add_overflow(va, size, &end) ||
	    check_add_overflow(km->high_va, km->size, &km_end))
		return false;
	if (va >= km->high_va && end <= km_end)
		return true;
	if (high_in_alias(km, va, end))
		return true;
	page_va = va & PAGE_MASK;
	last_page = (end - 1) & PAGE_MASK;
	for (;; page_va += PAGE_SIZE) {
		if (!(page_va >= km->high_va && page_va < km_end) &&
		    !high_overlaps_alias(km, page_va, page_va + PAGE_SIZE) &&
		    !xa_load(&km->dyn_slots, page_va >> PAGE_SHIFT))
			return false;
		if (page_va == last_page)
			break;
	}
	return true;
}

int fki_high_read_u64x2(unsigned long va, fki_u64 *out0, fki_u64 *out1)
{
	struct fki_kmem *km = fki_kmem_get();
	unsigned long flags;
	int err = -EFAULT;

	if (!km || !out0 || !out1 || (va & 7UL)) {
		fki_kmem_put(km);
		return -EFAULT;
	}
	spin_lock_irqsave(&km->lock, flags);
	if (high_range_readable_locked(km, va, 16)) {
		const volatile fki_u64 *p = (const volatile fki_u64 *)va;

		*out0 = p[0];
		*out1 = p[1];
		err = 0;
	}
	spin_unlock_irqrestore(&km->lock, flags);
	fki_kmem_put(km);
	return err;
}
EXPORT_SYMBOL(fki_high_read_u64x2);

static unsigned long icache_anchor;

void fki_icache_flush(unsigned long va, unsigned long size)
{
	struct fki_kmem *km = fki_kmem_get();

	if (km && size) {
		unsigned long flags;

		spin_lock_irqsave(&km->lock, flags);
		if (high_range_readable_locked(km, va, size)) {
			unsigned long ctr;

			asm volatile("mrs %0, ctr_el0" : "=r"(ctr));
			if (!((ctr >> 28) & 1UL)) {
				unsigned long step = 4UL << ((ctr >> 16) & 0xfUL);
				unsigned long cur = va & ~(step - 1);
				unsigned long end = va + size;

				for (; cur < end; cur += step)
					asm volatile("dc cvau, %0" : : "r"(cur) : "memory");
				asm volatile("dsb ish" ::: "memory");
			}
		}
		spin_unlock_irqrestore(&km->lock, flags);
	}
	fki_kmem_put(km);
	/* Trigger EL1 cross-core ic ialluis on a valid Linux low-VA anchor (never pass high VA or 0,0). */
	l4h_flush_icache((unsigned long)&icache_anchor, (unsigned long)&icache_anchor + 64);
	asm volatile("dsb ish; isb" ::: "memory");
}
EXPORT_SYMBOL(fki_icache_flush);

/* Use high window for fki_region_alloc so mode=1 smoke tests have executable memory. */
int fki_region_alloc(unsigned long size, struct fki_region *r)
{
	return fki_kmem_alloc(size, FKI_L4H_REGION_VA, FKI_L4H_REGION_PA, r);
}
EXPORT_SYMBOL(fki_region_alloc);

void fki_region_free(struct fki_region *r)
{
	fki_kmem_free(r);
}
EXPORT_SYMBOL(fki_region_free);

fki_u64 fki_phys_peek(fki_u64 pa)
{
	struct fki_kmem *km = fki_kmem_get();
	unsigned long gpa;
	fki_u64 val = ~0ULL;

	if (km && xpa_to_gpa(km, pa & ~7ULL, &gpa) == 0)
		val = READ_ONCE(*(u64 *)phys_to_virt(gpa));
	fki_kmem_put(km);
	return val;
}
EXPORT_SYMBOL(fki_phys_peek);

/* ---- EL1-only primitives (unsupported on EL0 host) ---- */

int fki_contig_alloc(unsigned long size, struct fki_region *r) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_contig_alloc);
void fki_contig_free(struct fki_region *r) {}
EXPORT_SYMBOL(fki_contig_free);
int fki_window_l1(unsigned long va, unsigned long long **l1_va, fki_u64 *l1_pa) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_window_l1);
void fki_window_set(unsigned long long *slot, fki_u64 desc) {}
EXPORT_SYMBOL(fki_window_set);
unsigned long fki_regs_pc(void *regs) { return 0; }
EXPORT_SYMBOL(fki_regs_pc);
unsigned long fki_regs_lr(void *regs) { return 0; }
EXPORT_SYMBOL(fki_regs_lr);
unsigned long fki_regs_pstate(void *regs) { return 0; }
EXPORT_SYMBOL(fki_regs_pstate);
void fki_regs_set_pc(void *regs, unsigned long pc) {}
EXPORT_SYMBOL(fki_regs_set_pc);
void fki_regs_set_reg(void *regs, unsigned int n, unsigned long v) {}
EXPORT_SYMBOL(fki_regs_set_reg);
unsigned long fki_regs_reg(void *regs, unsigned int n) { return 0; }
EXPORT_SYMBOL(fki_regs_reg);
int fki_fault_register(unsigned long lo, unsigned long hi, int (*handler)(unsigned long far, unsigned long esr, void *regs)) { return -EOPNOTSUPP; }
EXPORT_SYMBOL(fki_fault_register);
void fki_fault_unregister(void) {}
EXPORT_SYMBOL(fki_fault_unregister);

/* ---- User address space & uTCB primitives ---- */

struct fki_as {
	int vs_idx;
	u64 gen;
	atomic_t active_cpus;
	refcount_t refcnt;
	bool dead;
	spinlock_t lock;
	struct list_head kmem_node;
};

static bool fki_as_get(struct fki_as *as)
{
	return as && refcount_inc_not_zero(&as->refcnt);
}

static void fki_as_put(struct fki_as *as)
{
	if (as && refcount_dec_and_test(&as->refcnt))
		kfree(as);
}

struct fki_vs_slot {
	unsigned long root;
	struct fki_as *owner;
	struct list_head lru;
	struct list_head pts;
	struct xarray slots;
};

static struct fki_vs_slot *fki_vs_pool;
static int fki_vs_cap;
static LIST_HEAD(fki_vs_free);
static LIST_HEAD(fki_vs_lru);
static DEFINE_SPINLOCK(fki_vs_lock);
static int fki_vs_resetting;
static atomic64_t fki_vs_gen = ATOMIC64_INIT(1);

static void fki_vs_slot_reset(struct fki_vs_slot *slot)
{
	struct fki_pt_node *node, *tmp;
	unsigned long vpn;
	void *e;

	/* Free page tables in reverse order (highest level first) to avoid per-frame shootdowns. */
	list_for_each_entry_safe_reverse(node, tmp, &slot->pts, list) {
		list_del(&node->list);
		l4h_obj_free(&node->pt);
		kfree(node);
	}
	xa_for_each(&slot->slots, vpn, e) {
		unsigned long cptr = fki_as_entry_cptr(e);

		if (cptr)
			l4h_slot_delete(cptr);
	}
	xa_destroy(&slot->slots);
	xa_init(&slot->slots);
	l4h_vspace_flush(slot->root);
}

static int fki_vs_pool_init(void)
{
	int nr, cap, i;

	if (fki_vs_pool)
		return 0;
	nr = (int)l4h_bootinfo->nr_vspaces;
	cap = nr > 80 ? clamp(nr - 48, 32, 192) : (nr > 16 ? nr / 2 : 0);
	if (cap <= 0)
		return -ENOMEM;

	fki_vs_pool = kcalloc(cap, sizeof(*fki_vs_pool), GFP_KERNEL);
	if (!fki_vs_pool)
		return -ENOMEM;

	INIT_LIST_HEAD(&fki_vs_free);
	INIT_LIST_HEAD(&fki_vs_lru);
	fki_vs_resetting = 0;

	for (i = 0; i < cap; i++) {
		unsigned long root = l4h_vspace_get();

		if (!root) {
			fki_vs_cap = i;
			fki_vs_pool_fini();
			return -ENOMEM;
		}
		fki_vs_pool[i].root = root;
		fki_vs_pool[i].owner = NULL;
		INIT_LIST_HEAD(&fki_vs_pool[i].lru);
		INIT_LIST_HEAD(&fki_vs_pool[i].pts);
		xa_init(&fki_vs_pool[i].slots);
		list_add_tail(&fki_vs_pool[i].lru, &fki_vs_free);
	}
	fki_vs_cap = cap;
	pr_info("fki: reserved %d/%d user VSpaces for hosted user spaces\n", cap, nr);
	return 0;
}

static void fki_vs_pool_fini(void)
{
	int i;

	if (!fki_vs_pool)
		return;
	for (i = 0; i < fki_vs_cap; i++) {
		struct fki_vs_slot *slot = &fki_vs_pool[i];
		struct fki_as *owner = slot->owner;

		if (owner) {
			unsigned long flags;

			spin_lock_irqsave(&owner->lock, flags);
			WRITE_ONCE(owner->vs_idx, -1);
			owner->gen++;
			spin_unlock_irqrestore(&owner->lock, flags);
			slot->owner = NULL;
		}
		fki_vs_slot_reset(slot);
		if (owner)
			fki_as_put(owner);
		if (slot->root)
			l4h_vspace_put(slot->root);
	}
	kfree(fki_vs_pool);
	fki_vs_pool = NULL;
	fki_vs_cap = 0;
	INIT_LIST_HEAD(&fki_vs_free);
	INIT_LIST_HEAD(&fki_vs_lru);
}

static void fki_as_drain_list(struct fki_kmem *km)
{
	unsigned long flags;

	for (;;) {
		struct fki_as *as = NULL;

		spin_lock_irqsave(&fki_vs_lock, flags);
		if (!list_empty(&km->as_list)) {
			as = list_first_entry(&km->as_list, struct fki_as, kmem_node);
			list_del_init(&as->kmem_node);
			WRITE_ONCE(as->dead, true);
		}
		spin_unlock_irqrestore(&fki_vs_lock, flags);
		if (!as)
			break;
		fki_as_put(as);
	}
}

void *fki_as_create(void)
{
	struct fki_kmem *km = fki_kmem_get();
	struct fki_as *as;
	unsigned long flags;

	if (!km)
		return NULL;
	as = kzalloc(sizeof(*as), GFP_ATOMIC);
	if (!as) {
		fki_kmem_put(km);
		return NULL;
	}
	as->vs_idx = -1;
	as->gen = 0;
	atomic_set(&as->active_cpus, 0);
	refcount_set(&as->refcnt, 1);
	spin_lock_init(&as->lock);
	INIT_LIST_HEAD(&as->kmem_node);
	spin_lock_irqsave(&fki_vs_lock, flags);
	if (READ_ONCE(km->dead)) {
		spin_unlock_irqrestore(&fki_vs_lock, flags);
		kfree(as);
		fki_kmem_put(km);
		return NULL;
	}
	list_add_tail(&as->kmem_node, &km->as_list);
	spin_unlock_irqrestore(&fki_vs_lock, flags);
	fki_kmem_put(km);
	return as;
}
EXPORT_SYMBOL(fki_as_create);

void fki_as_destroy(void *as_ptr)
{
	struct fki_as *as = as_ptr;
	struct fki_kmem *km;
	struct fki_vs_slot *slot = NULL;
	unsigned long flags;
	bool drop_owner, drop_slot_ref = false;

	if (!as)
		return;
	km = fki_kmem_get();
	if (!km)
		return;
	spin_lock_irqsave(&fki_vs_lock, flags);
	drop_owner = !list_empty(&as->kmem_node);
	if (drop_owner)
		list_del_init(&as->kmem_node);
	spin_lock(&as->lock);
	WRITE_ONCE(as->dead, true);
	if (as->vs_idx >= 0 && fki_vs_pool) {
		slot = &fki_vs_pool[as->vs_idx];
		WRITE_ONCE(as->vs_idx, -1);
		as->gen++;
		if (slot->owner == as) {
			slot->owner = NULL;
			drop_slot_ref = true;
		}
		list_del_init(&slot->lru);
		fki_vs_resetting++;
	}
	spin_unlock(&as->lock);
	spin_unlock_irqrestore(&fki_vs_lock, flags);

	smp_mb();
	while (atomic_read(&as->active_cpus) != 0)
		cpu_relax();

	if (slot) {
		fki_vs_slot_reset(slot);
		spin_lock_irqsave(&fki_vs_lock, flags);
		fki_vs_resetting--;
		list_add(&slot->lru, &fki_vs_free);
		spin_unlock_irqrestore(&fki_vs_lock, flags);
	}
	fki_kmem_put(km);
	if (drop_slot_ref)
		fki_as_put(as);
	if (drop_owner)
		fki_as_put(as);
}
EXPORT_SYMBOL(fki_as_destroy);

static int map_user_frame(struct fki_vs_slot *slot, unsigned long cptr,
			  unsigned long va, unsigned long lprot)
{
	int err;

	while ((err = l4h_page_map(cptr, slot->root, va, (int)lprot)) == L4H_ERR_FAILED_LOOKUP) {
		struct fki_pt_node *node = kzalloc(sizeof(*node), GFP_ATOMIC);

		if (!node)
			return -ENOMEM;
		if (l4h_obj_alloc(L4H_OBJ_PT, 0, &node->pt)) {
			kfree(node);
			return -ENOMEM;
		}
		err = l4h_pt_map(L4H_OBJ_PT, node->pt.cptr, slot->root, va);
		if (err) {
			l4h_obj_free(&node->pt);
			kfree(node);
			return -EIO;
		}
		list_add(&node->list, &slot->pts);
	}
	return err ? -EIO : 0;
}

static void fki_as_unmap_km(struct fki_kmem *km, struct fki_as *as,
			    unsigned long va, unsigned long size);

int fki_as_map(void *as_ptr, unsigned long va, fki_u64 xpa, unsigned long size, int prot)
{
	struct fki_as *as = as_ptr;
	struct fki_kmem *km = fki_kmem_get();
	unsigned long lprot, va_end, flags, off;
	fki_u64 xpa_end;
	int changed = 0, err = 0;

	if (!as || !km) {
		fki_kmem_put(km);
		return -ENODEV;
	}
	if (READ_ONCE(as->dead)) {
		fki_kmem_put(km);
		return -EINVAL;
	}
	if ((va | xpa | size) & ~PAGE_MASK) {
		fki_kmem_put(km);
		return -EINVAL;
	}
	if (!size) {
		fki_kmem_put(km);
		return 0;
	}
	if (check_add_overflow(va, size, &va_end) ||
	    check_add_overflow(xpa, (fki_u64)size, &xpa_end)) {
		fki_kmem_put(km);
		return -EINVAL;
	}
	if ((prot & FKI_PROT_EXEC) && !(prot & FKI_PROT_READ)) {
		fki_kmem_put(km);
		return -EOPNOTSUPP;
	}
	lprot = fki_eff_prot(prot_to_l4h(prot));
	if (!lprot) {
		fki_as_unmap_km(km, as, va, size);
		fki_kmem_put(km);
		return 0;
	}

	spin_lock_irqsave(&as->lock, flags);
	if (READ_ONCE(as->dead)) {
		spin_unlock_irqrestore(&as->lock, flags);
		fki_kmem_put(km);
		return -EINVAL;
	}
	if (as->vs_idx < 0 || !fki_vs_pool) {
		spin_unlock_irqrestore(&as->lock, flags);
		fki_kmem_put(km);
		return 0;
	}
	{
		struct fki_vs_slot *slot = &fki_vs_pool[as->vs_idx];

		for (off = 0; off < size; off += PAGE_SIZE) {
			unsigned long cur_va = va + off;
			fki_u64 cur_xpa = xpa + off;
			unsigned long vpn = cur_va >> PAGE_SHIFT;
			unsigned long gpa, pfn, cptr;
			void *e;

			err = xpa_to_gpa(km, cur_xpa, &gpa);
			if (err)
				break;
			pfn = gpa >> PAGE_SHIFT;
			e = xa_load(&slot->slots, vpn);
			if (e && fki_as_entry_pfn(e) == pfn) {
				if (fki_as_entry_prot(e) == lprot)
					continue;
				cptr = fki_as_entry_cptr(e);
				if (l4h_page_map(cptr, slot->root, cur_va, (int)lprot) == 0) {
					xa_store(&slot->slots, vpn,
						 fki_as_mk_entry(cptr, pfn, lprot), GFP_ATOMIC);
					changed = 1;
					continue;
				}
				/* Remap failed: delete and recreate below. */
			}
			if (e) {
				unsigned long old_cptr = fki_as_entry_cptr(e);

				xa_erase(&slot->slots, vpn);
				if (old_cptr)
					l4h_slot_delete(old_cptr);
			}
			cptr = l4h_cap_copy(L4H_FRAME_CPTR(gpa), L4H_RIGHTS_ALL);
			if (!cptr) {
				err = -ENOMEM;
				break;
			}
			err = map_user_frame(slot, cptr, cur_va, lprot);
			if (err) {
				l4h_slot_delete(cptr);
				break;
			}
			if (xa_err(xa_store(&slot->slots, vpn,
					    fki_as_mk_entry(cptr, pfn, lprot), GFP_ATOMIC))) {
				l4h_slot_delete(cptr);
				err = -ENOMEM;
				break;
			}
			changed = 1;
		}
	}
	spin_unlock_irqrestore(&as->lock, flags);
	fki_kmem_put(km);
	return err ? err : changed;
}
EXPORT_SYMBOL(fki_as_map);

static void fki_vs_slot_unmap_range(struct fki_vs_slot *slot,
				    unsigned long va, unsigned long size)
{
	unsigned long first_vpn, last_va, last_vpn, vpn;
	void *e;

	if (!size)
		return;
	first_vpn = va >> PAGE_SHIFT;
	last_va = (size - 1 > ULONG_MAX - va) ? ULONG_MAX : (va + size - 1);
	last_vpn = last_va >> PAGE_SHIFT;
	xa_for_each_range(&slot->slots, vpn, e, first_vpn, last_vpn) {
		unsigned long cptr = fki_as_entry_cptr(e);

		xa_erase(&slot->slots, vpn);
		if (cptr)
			l4h_slot_delete(cptr);
	}
}

static void fki_as_unmap_km(struct fki_kmem *km, struct fki_as *as,
			    unsigned long va, unsigned long size)
{
	unsigned long flags, va_end;

	if (!km || !size)
		return;
	if ((va | size) & ~PAGE_MASK)
		return;
	if (check_add_overflow(va, size, &va_end))
		return;
	if (!as) {
		int i;

		if (!fki_vs_pool)
			return;
		spin_lock_irqsave(&fki_vs_lock, flags);
		for (i = 0; i < fki_vs_cap; i++) {
			struct fki_vs_slot *slot = &fki_vs_pool[i];
			struct fki_as *owner = slot->owner;

			if (!owner)
				continue;
			spin_lock(&owner->lock);
			fki_vs_slot_unmap_range(slot, va, size);
			spin_unlock(&owner->lock);
		}
		spin_unlock_irqrestore(&fki_vs_lock, flags);
		return;
	}
	spin_lock_irqsave(&as->lock, flags);
	if (as->vs_idx >= 0 && fki_vs_pool)
		fki_vs_slot_unmap_range(&fki_vs_pool[as->vs_idx], va, size);
	spin_unlock_irqrestore(&as->lock, flags);
}

void fki_as_unmap(void *as_ptr, unsigned long va, unsigned long size)
{
	struct fki_kmem *km = fki_kmem_get();

	if (!km)
		return;
	fki_as_unmap_km(km, as_ptr, va, size);
	fki_kmem_put(km);
}
EXPORT_SYMBOL(fki_as_unmap);

void fki_as_flush(void *as_ptr)
{
	struct fki_as *as = as_ptr;
	struct fki_kmem *km;
	unsigned long flags;

	if (!as)
		return;
	km = fki_kmem_get();
	if (!km)
		return;
	spin_lock_irqsave(&as->lock, flags);
	if (as->vs_idx >= 0 && fki_vs_pool)
		l4h_vspace_flush(fki_vs_pool[as->vs_idx].root);
	spin_unlock_irqrestore(&as->lock, flags);
	fki_kmem_put(km);
}
EXPORT_SYMBOL(fki_as_flush);

/*
 * Caller must have incremented as->active_cpus before calling.
 * Note: fast-path hits in fki_utcb_run_inner do not take fki_vs_lock or move
 * the LRU node, so eviction order is approximate LRU.
 */
static struct fki_vs_slot *fki_vs_activate(struct fki_as *as, u64 *gen_out)
{
	struct fki_vs_slot *slot, *victim;
	unsigned long flags;

	if (!fki_vs_pool)
		return NULL;

	for (;;) {
		spin_lock_irqsave(&fki_vs_lock, flags);
		if (READ_ONCE(as->dead)) {
			spin_unlock_irqrestore(&fki_vs_lock, flags);
			return NULL;
		}
		if (as->vs_idx >= 0) {
			slot = &fki_vs_pool[as->vs_idx];
			list_move_tail(&slot->lru, &fki_vs_lru);
			*gen_out = as->gen;
			spin_unlock_irqrestore(&fki_vs_lock, flags);
			return slot;
		}
		if (!list_empty(&fki_vs_free)) {
			slot = list_first_entry(&fki_vs_free, struct fki_vs_slot, lru);
			list_del_init(&slot->lru);
			spin_lock(&as->lock);
			if (!fki_as_get(as)) {
				spin_unlock(&as->lock);
				list_add(&slot->lru, &fki_vs_free);
				spin_unlock_irqrestore(&fki_vs_lock, flags);
				return NULL;
			}
			slot->owner = as;
			as->gen = (u64)atomic64_inc_return(&fki_vs_gen);
			*gen_out = as->gen;
			smp_store_release(&as->vs_idx, (int)(slot - fki_vs_pool));
			spin_unlock(&as->lock);
			list_add_tail(&slot->lru, &fki_vs_lru);
			spin_unlock_irqrestore(&fki_vs_lock, flags);
			return slot;
		}
		victim = NULL;
		list_for_each_entry(slot, &fki_vs_lru, lru) {
			if (slot->owner && atomic_read(&slot->owner->active_cpus) == 0) {
				victim = slot;
				break;
			}
		}
		if (victim) {
			struct fki_as *vas = victim->owner;
			int old_idx;

			spin_lock(&vas->lock);
			old_idx = vas->vs_idx;
			WRITE_ONCE(vas->vs_idx, -1);
			smp_mb();
			if (atomic_read(&vas->active_cpus) != 0) {
				smp_store_release(&vas->vs_idx, old_idx);
				spin_unlock(&vas->lock);
				spin_unlock_irqrestore(&fki_vs_lock, flags);
				cpu_relax();
				continue;
			}
			vas->gen++;
			victim->owner = NULL;
			list_del_init(&victim->lru);
			fki_vs_resetting++;
			spin_unlock(&vas->lock);
			spin_unlock_irqrestore(&fki_vs_lock, flags);

			fki_as_put(vas);
			fki_vs_slot_reset(victim);

			spin_lock_irqsave(&fki_vs_lock, flags);
			fki_vs_resetting--;
			list_add(&victim->lru, &fki_vs_free);
			spin_unlock_irqrestore(&fki_vs_lock, flags);
			continue;
		}
		if (!fki_vs_resetting) {
			spin_unlock_irqrestore(&fki_vs_lock, flags);
			pr_err("fki: all %d user VSpaces are pinned by active CPUs\n", fki_vs_cap);
			return NULL;
		}
		spin_unlock_irqrestore(&fki_vs_lock, flags);
		cpu_relax();
	}
}

struct fki_utcb {
	struct l4h_uctx u;
	struct l4h_obj tcb_obj;
	struct l4h_obj reply_obj;
	struct fki_as *as;
	u64 as_gen;
	int cpu;
	bool have_tls;
	u64 last_tpidr;
	u64 last_tpidrro;
	bool have_pac;
	struct fki_pac_keys last_pac;
};

void *fki_utcb_create(void *as_ptr)
{
	struct fki_utcb *utcb;
	struct l4h_uctx *u;
	struct fki_as *as = as_ptr;
	int cpu = smp_processor_id();

	if (as) {
		unsigned long flags;
		bool ok;

		spin_lock_irqsave(&as->lock, flags);
		ok = !READ_ONCE(as->dead) && fki_as_get(as);
		spin_unlock_irqrestore(&as->lock, flags);
		if (!ok)
			return NULL;
	}

	utcb = kzalloc(sizeof(*utcb), GFP_ATOMIC);
	if (!utcb) {
		fki_as_put(as);
		return NULL;
	}
	u = &utcb->u;

	if (l4h_obj_alloc(L4H_OBJ_TCB, 0, &utcb->tcb_obj))
		goto fail_free;
	if (l4h_obj_alloc(L4H_OBJ_REPLY, 0, &utcb->reply_obj))
		goto fail_tcb;
	u->fault_ep = l4h_slot_alloc();
	if (!u->fault_ep)
		goto fail_reply;

	u->tcb = utcb->tcb_obj.cptr;
	u->reply = utcb->reply_obj.cptr;
	u->badge = l4h_uctx_alloc_badge();
	if (l4h_cnode_mint(u->fault_ep, 64, L4H_SLOT_VCPU(cpu) + L4H_VCPU_EP,
			   L4H_ROOT_DEPTH, L4H_RIGHTS_ALL, u->badge))
		goto fail_ep;
	if (l4h_tcb_set_flags(u->tcb, 1 /* fpuDisabled */, 2 /* trapSyscalls */))
		goto fail_ep;

	u->fault_ep_vcpu = cpu;
	u->sc_vcpu = -1;
	u->vspace = 0;
	u->state = L4H_UCTX_FRESH;
	u->regs_valid = 1;
	utcb->cpu = cpu;
	utcb->as = as;
	return utcb;

fail_ep:
	l4h_slot_delete(u->fault_ep);
fail_reply:
	l4h_obj_free(&utcb->reply_obj);
fail_tcb:
	l4h_obj_free(&utcb->tcb_obj);
fail_free:
	fki_as_put(as);
	kfree(utcb);
	return NULL;
}
EXPORT_SYMBOL(fki_utcb_create);

static void fki_utcb_desched_ipi(void *arg)
{
	struct l4h_uctx *u = arg;

	if (this_cpu_read(fki_cur_utcb) == u)
		this_cpu_write(fki_cur_utcb, NULL);
	l4h_user_deschedule(u);
}

void fki_utcb_destroy(void *utcb_ptr)
{
	struct fki_utcb *utcb = utcb_ptr;
	struct l4h_uctx *u;

	if (!utcb)
		return;
	u = &utcb->u;
	if (u->sc_vcpu >= 0) {
		if (u->sc_vcpu == smp_processor_id()) {
			fki_utcb_desched_ipi(u);
		} else if (u->sc_vcpu < nr_cpu_ids && cpu_online(u->sc_vcpu)) {
			smp_call_function_single(u->sc_vcpu, fki_utcb_desched_ipi, u, 1);
		}
	}
	if (u->fault_ep)
		l4h_slot_delete(u->fault_ep);
	fki_as_put(utcb->as);
	utcb->as = NULL;
	l4h_obj_free(&utcb->reply_obj);
	l4h_obj_free(&utcb->tcb_obj);
	kfree(utcb);
}
EXPORT_SYMBOL(fki_utcb_destroy);

int fki_utcb_set_as(void *utcb_ptr, void *as_ptr)
{
	struct fki_utcb *utcb = utcb_ptr;
	struct fki_as *new_as = as_ptr;
	struct fki_as *old_as;
	unsigned long flags;
	bool ok;

	if (!utcb || !new_as)
		return -EINVAL;
	if (utcb->as == new_as)
		return READ_ONCE(new_as->dead) ? -EINVAL : 0;
	spin_lock_irqsave(&new_as->lock, flags);
	ok = !READ_ONCE(new_as->dead) && fki_as_get(new_as);
	spin_unlock_irqrestore(&new_as->lock, flags);
	if (!ok)
		return -EINVAL;
	old_as = utcb->as;
	utcb->as = new_as;
	fki_as_put(old_as);
	return 0;
}
EXPORT_SYMBOL(fki_utcb_set_as);

int fki_utcb_set_pac(void *utcb_ptr, const struct fki_pac_keys *keys)
{
	struct fki_utcb *utcb = utcb_ptr;
	unsigned long words[L4H_PAC_KEY_WORDS];
	unsigned long en;
	int err;

	if (!utcb || !keys)
		return -EINVAL;
	en = sctlr_to_l4h_pac_en(keys->sctlr_en);
	if (!(l4h_provider_features() & L4H_PROVIDER_ARM_PAC)) {
		if (en)
			return -EOPNOTSUPP;
		utcb->last_pac = *keys;
		utcb->have_pac = true;
		return 0;
	}
	if (utcb->have_pac && memcmp(&utcb->last_pac, keys, sizeof(*keys)) == 0)
		return 0;

	if (!en) {
		err = l4h_tcb_set_pac(utcb->u.tcb, 0, NULL);
	} else {
		words[0] = keys->ia[0]; words[1] = keys->ia[1];
		words[2] = keys->ib[0]; words[3] = keys->ib[1];
		words[4] = keys->da[0]; words[5] = keys->da[1];
		words[6] = keys->db[0]; words[7] = keys->db[1];
		words[8] = keys->ga[0]; words[9] = keys->ga[1];
		err = l4h_tcb_set_pac(utcb->u.tcb, en, words);
	}
	if (err == FKI_L4H_ERR_ILLEGAL_OP)
		return -EOPNOTSUPP;
	if (err)
		return -EIO;
	utcb->last_pac = *keys;
	utcb->have_pac = true;
	return 0;
}
EXPORT_SYMBOL(fki_utcb_set_pac);

static int fki_utcb_run_inner(void *utcb_ptr, struct fki_utcb_regs *regs,
			      struct fki_utcb_stop *stop, bool resume_unchanged)
{
	struct fki_utcb *utcb = utcb_ptr;
	struct fki_kmem *km;
	struct l4h_uctx *u;
	struct fki_as *as;
	struct fki_vs_slot *slot;
	struct l4h_user_ext ext;
	unsigned long gp[37];
	u64 gen = 0;
	bool as_changed, tls_ro_changed, fast_resume;
	int st, i, idx;

	if (!utcb || !regs || !stop || !utcb->as)
		return -EINVAL;
	km = fki_kmem_get();
	if (!km)
		return -EINVAL;
	u = &utcb->u;
	as = utcb->as;

	atomic_inc(&as->active_cpus);
	smp_mb__after_atomic();
	if (READ_ONCE(as->dead)) {
		atomic_dec(&as->active_cpus);
		fki_kmem_put(km);
		return -EINVAL;
	}
	idx = smp_load_acquire(&as->vs_idx);
	if (likely(idx >= 0 && fki_vs_pool)) {
		gen = READ_ONCE(as->gen);
		slot = &fki_vs_pool[idx];
	} else {
		slot = fki_vs_activate(as, &gen);
		if (!slot) {
			bool is_dead = READ_ONCE(as->dead);

			atomic_dec(&as->active_cpus);
			fki_kmem_put(km);
			return is_dead ? -EINVAL : -ENOMEM;
		}
	}

	as_changed = (u->vspace != slot->root || utcb->as_gen != gen);
	if (as_changed) {
		int err;

		if (u->state == L4H_UCTX_FRESH && !u->vspace)
			err = l4h_tcb_configure_user(u->tcb, l4h_empty_cnode_cptr(), slot->root);
		else
			err = l4h_tcb_set_space(u->tcb, u->fault_ep, l4h_empty_cnode_cptr(), slot->root);
		if (err) {
			atomic_dec(&as->active_cpus);
			fki_kmem_put(km);
			pr_err("fki: binding user TCB to VSpace failed: %d\n", err);
			return -EIO;
		}
		u->vspace = slot->root;
		utcb->as_gen = gen;
	}

	fast_resume = (resume_unchanged && !as_changed && !u->regs_valid &&
		       u->state == L4H_UCTX_FAULTED &&
		       utcb->have_tls && regs->tpidrro == utcb->last_tpidrro);
	if (!fast_resume) {
		if (resume_unchanged && !u->regs_valid) {
			/*
			 * Stopped on a page fault with live GP registers still in the
			 * TCB, but could not take fast_resume (e.g. VSpace eviction
			 * forced a rebind): read the real TCB state instead of
			 * overwriting it with stale regs[].
			 */
			l4h_user_sync_regs(u, gp);
			gp[34] = 0;
			gp[36] = 0;
		} else {
			for (i = 0; i < 31; i++)
				gp[i] = (unsigned long)regs->x[i];
			gp[31] = (unsigned long)regs->sp;
			gp[32] = (unsigned long)regs->pc;
			gp[33] = (unsigned long)(regs->pstate & 0xf0000000UL);
			gp[34] = 0;
			gp[L4H_GP_TLS] = (unsigned long)regs->tpidr;
			gp[36] = 0;
		}

		tls_ro_changed = (!utcb->have_tls ||
				  regs->tpidrro != utcb->last_tpidrro ||
				  as_changed ||
				  u->state == L4H_UCTX_FRESH);
		l4h_user_mark_dirty(u, tls_ro_changed ? 1 : 0);
	} else {
		tls_ro_changed = false;
	}

	ext.size = sizeof(ext);
	ext.flags = L4H_USER_EXT_TLS_RO |
		    (tls_ro_changed ? L4H_USER_EXT_TLS_RO_UPDATE : 0);
	ext.tls_ro = (unsigned long)regs->tpidrro;
	ext.raw_fsr = 0;

	this_cpu_write(fki_cur_utcb, u);
	st = l4h_user_run_ex(u, gp, 1, &ext);
	if (st == L4H_STOP_PAGEFAULT) {
		atomic_dec(&as->active_cpus);
		fki_kmem_put(km);
		utcb->last_tpidrro = regs->tpidrro;
		utcb->have_tls = true;
		stop->kind = FKI_STOP_PAGEFAULT;
		stop->esr = ext.raw_fsr;
		stop->fault_addr = u->fault_addr;
		stop->fault_ip = u->fault_ip;
		return 0;
	}
	if (!u->regs_valid)
		l4h_user_sync_regs(u, gp);

	atomic_dec(&as->active_cpus);
	fki_kmem_put(km);

	for (i = 0; i < 31; i++)
		regs->x[i] = gp[i];
	regs->sp = gp[31];
	regs->pc = gp[32];
	regs->pstate = gp[33];
	regs->tpidr = gp[L4H_GP_TLS];
	utcb->last_tpidr = regs->tpidr;
	utcb->last_tpidrro = regs->tpidrro;
	utcb->have_tls = true;

	switch (st) {
	case L4H_STOP_SYSCALL:
		stop->kind = FKI_STOP_SYSCALL;
		stop->esr = (0x15ULL << 26) | (1ULL << 25) | 0x80ULL;
		stop->fault_ip = gp[32];
		stop->fault_addr = 0;
		break;
	case L4H_STOP_EXCEPTION:
		stop->kind = FKI_STOP_EXCEPTION;
		stop->esr = u->exc_number;
		stop->fault_addr = u->fault_addr;
		stop->fault_ip = u->fault_ip;
		break;
	case L4H_STOP_INTERRUPT:
		stop->kind = FKI_STOP_INTERRUPT;
		stop->esr = ~0ULL;
		stop->fault_addr = 0;
		stop->fault_ip = gp[32];
		break;
	default:
		return -EIO;
	}
	return 0;
}

int fki_utcb_run(void *utcb_ptr, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop)
{
	return fki_utcb_run_inner(utcb_ptr, regs, stop, false);
}
EXPORT_SYMBOL(fki_utcb_run);

int fki_utcb_resume(void *utcb_ptr, struct fki_utcb_regs *regs, struct fki_utcb_stop *stop)
{
	return fki_utcb_run_inner(utcb_ptr, regs, stop, true);
}
EXPORT_SYMBOL(fki_utcb_resume);

int fki_utcb_sync_regs(void *utcb_ptr, struct fki_utcb_regs *regs)
{
	struct fki_utcb *utcb = utcb_ptr;
	struct l4h_uctx *u;
	unsigned long gp[37];
	int i;

	if (!utcb || !regs)
		return -EINVAL;
	u = &utcb->u;
	if (u->regs_valid)
		return 0;
	l4h_user_sync_regs(u, gp);
	for (i = 0; i < 31; i++)
		regs->x[i] = gp[i];
	regs->sp = gp[31];
	regs->pc = gp[32];
	regs->pstate = gp[33];
	regs->tpidr = gp[L4H_GP_TLS];
	utcb->last_tpidr = regs->tpidr;
	return 0;
}
EXPORT_SYMBOL(fki_utcb_sync_regs);

int fki_utcb_fpu_read(void *utcb_ptr, void *buf, unsigned long len)
{
	struct fki_utcb *utcb = utcb_ptr;

	if (!utcb || !buf || !len)
		return -EINVAL;
	if (utcb->u.state == L4H_UCTX_FRESH) {
		memset(buf, 0, len);
		return 0;
	}
	return l4h_fpu_read(&utcb->u, buf, len) ? -EIO : 0;
}
EXPORT_SYMBOL(fki_utcb_fpu_read);

int fki_utcb_fpu_write(void *utcb_ptr, const void *buf, unsigned long len)
{
	struct fki_utcb *utcb = utcb_ptr;

	if (!utcb || !buf || !len)
		return -EINVAL;
	return l4h_fpu_write(&utcb->u, buf, len) ? -EIO : 0;
}
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

	for_each_possible_cpu(cpu) {
		struct page *pg = alloc_pages(GFP_KERNEL, FKI_ALT_STACK_ORDER);

		if (!pg) {
			fki_free_alt_stacks();
			return -ENOMEM;
		}
		fki_alt_stack[cpu] = page_address(pg);
	}
	return 0;
}

void fki_arch_exit(void)
{
	fki_free_alt_stacks();
}

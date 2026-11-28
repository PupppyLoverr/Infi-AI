// SPDX-License-Identifier: GPL-2.0-only
/*
 * Foreign kernel interface, version 0: see include/linux/fki.h.
 *
 * Exports are plain EXPORT_SYMBOL on purpose: this file defines a narrow,
 * stable, semantic interface of its own, and nothing behind it is
 * re-exported.
 */
#include <linux/fki.h>
#include <linux/firmware.h>
#include <linux/gfp.h>
#include <linux/irqflags.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/preempt.h>
#include <linux/printk.h>
#include <linux/random.h>
#include <linux/slab.h>
#include <linux/sched.h>
#include <linux/cpumask.h>
#include <linux/uaccess.h>
#include <linux/vmalloc.h>
#include <asm/arch_timer.h>
#include <asm/sysreg.h>

int fki_version(void)
{
	/* This minimal in-tree driver implements only the FKI v0 subset; full FKI v1 lives in fki-ko/. */
	return 0;
}
EXPORT_SYMBOL(fki_version);

void fki_log(const char *s, unsigned long len)
{
	pr_info("fki: %.*s\n", (int)len, s);
}
EXPORT_SYMBOL(fki_log);

int fki_region_alloc(unsigned long size, struct fki_region *r)
{
	unsigned long n = PAGE_ALIGN(size) >> PAGE_SHIFT, i;
	struct page *first, **pages;
	void *va;

	first = alloc_contig_pages(n, GFP_KERNEL, numa_node_id(), NULL);
	if (!first)
		return -ENOMEM;
	pages = kvmalloc_array(n, sizeof(*pages), GFP_KERNEL);
	if (!pages) {
		free_contig_range(page_to_pfn(first), n);
		return -ENOMEM;
	}
	for (i = 0; i < n; i++)
		pages[i] = first + i;
	va = vmap(pages, n, VM_MAP, PAGE_KERNEL_EXEC);
	kvfree(pages);
	if (!va) {
		free_contig_range(page_to_pfn(first), n);
		return -ENOMEM;
	}
	memset(va, 0, n << PAGE_SHIFT);
	r->va = va;
	r->pa = page_to_phys(first);
	r->size = n << PAGE_SHIFT;
	r->handle = first;
	return 0;
}
EXPORT_SYMBOL(fki_region_alloc);

void fki_region_free(struct fki_region *r)
{
	vunmap(r->va);
	free_contig_range(page_to_pfn((struct page *)r->handle), r->size >> PAGE_SHIFT);
}
EXPORT_SYMBOL(fki_region_free);

int fki_blob_get(const char *name, const void **data, unsigned long *size, void **handle)
{
	const struct firmware *fw;
	int err = request_firmware(&fw, name, NULL);

	if (err)
		return err;
	*data = fw->data;
	*size = fw->size;
	*handle = (void *)fw;
	return 0;
}
EXPORT_SYMBOL(fki_blob_get);

void fki_blob_put(void *handle)
{
	release_firmware(handle);
}
EXPORT_SYMBOL(fki_blob_put);

#define FKI_SCTLR_EN (SCTLR_ELx_ENIA | SCTLR_ELx_ENIB | SCTLR_ELx_ENDA | SCTLR_ELx_ENDB)


static void keys_read(struct fki_pac_keys *k)
{
	k->ia[0] = read_sysreg_s(SYS_APIAKEYLO_EL1); k->ia[1] = read_sysreg_s(SYS_APIAKEYHI_EL1);
	k->ib[0] = read_sysreg_s(SYS_APIBKEYLO_EL1); k->ib[1] = read_sysreg_s(SYS_APIBKEYHI_EL1);
	k->da[0] = read_sysreg_s(SYS_APDAKEYLO_EL1); k->da[1] = read_sysreg_s(SYS_APDAKEYHI_EL1);
	k->db[0] = read_sysreg_s(SYS_APDBKEYLO_EL1); k->db[1] = read_sysreg_s(SYS_APDBKEYHI_EL1);
	k->ga[0] = read_sysreg_s(SYS_APGAKEYLO_EL1); k->ga[1] = read_sysreg_s(SYS_APGAKEYHI_EL1);
	k->sctlr_en = read_sysreg(sctlr_el1) & FKI_SCTLR_EN;
}

static void keys_write(const struct fki_pac_keys *k)
{
	write_sysreg_s(k->ia[0], SYS_APIAKEYLO_EL1); write_sysreg_s(k->ia[1], SYS_APIAKEYHI_EL1);
	write_sysreg_s(k->ib[0], SYS_APIBKEYLO_EL1); write_sysreg_s(k->ib[1], SYS_APIBKEYHI_EL1);
	write_sysreg_s(k->da[0], SYS_APDAKEYLO_EL1); write_sysreg_s(k->da[1], SYS_APDAKEYHI_EL1);
	write_sysreg_s(k->db[0], SYS_APDBKEYLO_EL1); write_sysreg_s(k->db[1], SYS_APDBKEYHI_EL1);
	write_sysreg_s(k->ga[0], SYS_APGAKEYLO_EL1); write_sysreg_s(k->ga[1], SYS_APGAKEYHI_EL1);
	sysreg_clear_set(sctlr_el1, FKI_SCTLR_EN, k->sctlr_en & FKI_SCTLR_EN);
	isb();
}

unsigned long fki_enter(const struct fki_pac_keys *foreign, struct fki_pac_keys *saved)
{
	unsigned long flags;

	preempt_disable();
	local_irq_save(flags);
	keys_read(saved);
	keys_write(foreign);
	return flags;
}
EXPORT_SYMBOL(fki_enter);

void fki_leave(unsigned long cookie, const struct fki_pac_keys *saved)
{
	keys_write(saved);
	local_irq_restore(cookie);
	preempt_enable();
}
EXPORT_SYMBOL(fki_leave);

int fki_pac_swap(const struct fki_pac_keys *load, struct fki_pac_keys *save)
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

void fki_random(void *buf, unsigned long len)
{
	get_random_bytes(buf, len);
}
EXPORT_SYMBOL(fki_random);

int fki_random_wait(void)
{
	return wait_for_random_bytes();
}
EXPORT_SYMBOL(fki_random_wait);

int fki_bind_cpu(int cpu)
{
	if (cpu < 0 || cpu >= nr_cpu_ids || !cpu_online(cpu))
		return -EINVAL;
	return set_cpus_allowed_ptr(current, cpumask_of(cpu));
}
EXPORT_SYMBOL(fki_bind_cpu);

/* FKI v1 capability & EL0/high-window stubs (native EL1 backend). */
unsigned long fki_caps(void)
{
	return FKI_CAP_PAC | FKI_CAP_EXEC_ONLY;
}
EXPORT_SYMBOL(fki_caps);

int fki_cpu_id(void)
{
	return smp_processor_id();
}
EXPORT_SYMBOL(fki_cpu_id);

void fki_log_alert(const char *s, unsigned long len)
{
	pr_alert("fki: %.*s\n", (int)len, s);
}
EXPORT_SYMBOL(fki_log_alert);

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

/* Force-included compat shim — build infrastructure for newer/older host
   kernels, NOT a modification of xnulinux sources. */

/* 1. xnulinux passes a vestigial &scratch argument to kernel_neon_begin/end,
      valid on kernels whose prototypes had unspecified params; drop the
      argument where they are declared (void). */
#define kernel_neon_begin(x) kernel_neon_begin()
#define kernel_neon_end(x) kernel_neon_end()

/* 2. follow_pfnmap_start/end (>= 6.10) do not exist in Ubuntu 6.8; emulate
      them with follow_pte, which provides the same functionality. */
#include <linux/mm.h>
#include <asm/pgtable.h>

struct follow_pfnmap_args {
	struct vm_area_struct *vma;
	unsigned long address;
	unsigned long pfn;
	spinlock_t *ptlp;
	pte_t *ptep;
};

static inline int follow_pfnmap_start(struct follow_pfnmap_args *args)
{
	pte_t *ptep;
	spinlock_t *ptlp;
	int err = follow_pte(args->vma->vm_mm, args->address, &ptep, &ptlp);
	if (err)
		return err;
	args->pfn = pte_pfn(*ptep);
	args->ptep = ptep;
	args->ptlp = ptlp;
	return 0;
}

static inline void follow_pfnmap_end(struct follow_pfnmap_args *args)
{
	pte_unmap_unlock(args->ptep, args->ptlp);
}

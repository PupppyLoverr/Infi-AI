// SPDX-License-Identifier: APSL-2.0
// PureDarwin's pthread and AMFI kexts build with XNU_KERNEL_PRIVATE (they reach into kernel structures), so their typed
// allocations call the kernel-internal kalloc_type_impl/kfree_type_impl, which a kext cannot link against: these forward
// them to the entry points the kernel exports to kexts.
#include <stdint.h>

extern void *kalloc_type_impl_external(const void *kt_view, uint32_t flags);
extern void kfree_type_impl_external(const void *kt_view, void *ptr);

void *kalloc_type_impl(const void *kt_view, uint32_t flags);
void kfree_type_impl(const void *kt_view, void *ptr);

void *
kalloc_type_impl(const void *kt_view, uint32_t flags)
{
	return kalloc_type_impl_external(kt_view, flags);
}

void
kfree_type_impl(const void *kt_view, void *ptr)
{
	kfree_type_impl_external(kt_view, ptr);
}

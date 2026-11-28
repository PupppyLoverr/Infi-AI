// SPDX-License-Identifier: APSL-2.0
// kmod start and stop for the free kexts whose work is done by their IOKit classes (static constructors run by OSKext):
// scripts/kclink/kclink.py --native wants a start and a stop routine in every kext it links from objects.
#include <mach/kmod.h>

kern_return_t native_kext_start(kmod_info_t *ki, void *d);
kern_return_t native_kext_stop(kmod_info_t *ki, void *d);

kern_return_t
native_kext_start(kmod_info_t *ki, void *d)
{
	(void)ki; (void)d;
	return KERN_SUCCESS;
}

kern_return_t
native_kext_stop(kmod_info_t *ki, void *d)
{
	(void)ki; (void)d;
	return KERN_SUCCESS;
}

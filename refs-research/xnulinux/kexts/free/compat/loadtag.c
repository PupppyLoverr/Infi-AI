// SPDX-License-Identifier: APSL-2.0
// What libkmod gives a kext linked as a bundle: its load tag, from its own kmod_info. A kext linked into the collection
// from objects has no libkmod; hfs only uses the tag to hold itself loaded while a volume is mounted, and a kext of the
// boot collection never unloads, so there is no tag to give (kOSKextInvalidLoadTag: the retain and release are refused,
// which changes nothing).
#include <stdint.h>

uint32_t OSKextGetCurrentLoadTag(void);

uint32_t
OSKextGetCurrentLoadTag(void)
{
	return (uint32_t)-1;
}

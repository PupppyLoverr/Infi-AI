// SPDX-License-Identifier: APSL-2.0
// The OSMalloc KPI (libkern/OSMalloc.h) that PureDarwin's IOStorageFamily still calls and this kernel no longer
// implements: a tag is a small allocation of its own, memory is IOMallocData. Tags are opaque to the callers, so the
// header (whose types this kernel does not export to kexts any more) is not needed.
#include <IOKit/IOLib.h>

#define TAG_SIZE 64

void *OSMalloc_Tagalloc(const char *name, uint32_t flags);
void OSMalloc_Tagfree(void *tag);
void *OSMalloc(uint32_t size, void *tag);
void *OSMalloc_noblock(uint32_t size, void *tag);
void OSFree(void *addr, uint32_t size, void *tag);

void *
OSMalloc_Tagalloc(const char *name, uint32_t flags)
{
	(void)name; (void)flags;
	return IOMallocZeroData(TAG_SIZE);
}

void
OSMalloc_Tagfree(void *tag)
{
	IOFreeData(tag, TAG_SIZE);
}

void *
OSMalloc(uint32_t size, void *tag)
{
	(void)tag;
	return IOMallocData(size);
}

void *
OSMalloc_noblock(uint32_t size, void *tag)
{
	(void)tag;
	return IOMallocData(size);
}

void
OSFree(void *addr, uint32_t size, void *tag)
{
	(void)tag;
	IOFreeData(addr, size);
}

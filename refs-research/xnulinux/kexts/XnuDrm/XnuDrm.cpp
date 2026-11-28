// SPDX-License-Identifier: APSL-2.0
// XnuDrm: the hosted macOS's door to the Linux host's GPU (the DRM bridge, gpu/xnu-drm). A process opens a user client
// (libxnudrm, inside Mesa's Venus driver); each of its requests is one host call (linux64_hostcall: LINUX64_CALL_HOSTCALL, xnu.ko,
// fki) answered by xnudrmd, the Linux service that owns the DRM render node. Buffer mappings come back as the physical
// pages Linux mapped, which this kext maps into the calling task: the process and the GPU share the memory, no copies.
//
// Processes open the user client on the Metal accelerator service (kexts/XnuCpuMetal names it as its IOUserClientClass):
// the sandbox profiles of processes that draw allow user clients of an IOAccelerator (system.sb's system-graphics) and
// refuse others ("IOUC ... failed MACF"). The class is named AGXDeviceUserClient, Apple GPUs' device user client, which
// the profiles that list classes by name admit too; no AGX kext is in the hosted collection, so the name is free. The
// XnuDrm service itself only says the bridge is there.
//
// Methods (IOConnectCallScalarMethod):
//   0 call(in_ptr, in_len, out_ptr, out_max) -> reply_len   a message for xnudrmd (xnudrm_proto.h), client id filled in
//   1 map(gem_handle, size) -> (user address, map id)        XNUDRM_MAP, then the pages mapped into the task
//   2 unmap(map id)
#include <IOKit/IOService.h>
#include <IOKit/IOUserClient.h>
#include <IOKit/IOMemoryDescriptor.h>
#include <IOKit/IOLib.h>
#include <IOKit/IOLocks.h>
#include <mach/kmod.h>
#include <sys/errno.h>
#include <libkern/copyio.h>
#include "../../gpu/xnu-drm/xnudrm_proto.h"

extern "C" long linux64_hostcall(const void *req, unsigned long len, void *reply, unsigned long max);

static IOLock *gCallLock;	// one host call at a time (the bounce buffers are shared)
static uint8_t *gIn, *gOut;	// XNUDRM_MAX_MSG + header each

// A message to xnudrmd and its reply (both in gIn/gOut, under gCallLock). Returns the reply's length or -errno.
static long hostcall(unsigned long len)
{
	long r = linux64_hostcall(gIn, len, gOut, sizeof(xnudrm_hdr) + XNUDRM_MAX_MSG);
	if (r >= 0 && ((size_t)r < sizeof(xnudrm_hdr) || ((xnudrm_hdr *)gOut)->magic != XNUDRM_MAGIC)) r = -EPROTO;
	return r;
}

class XnuDrm : public IOService {
	OSDeclareDefaultStructors(XnuDrm)
public:
	bool start(IOService *provider) override;
};

class AGXDeviceUserClient : public IOUserClient {
	OSDeclareDefaultStructors(AGXDeviceUserClient)
	task_t fTask = nullptr;
	uint32_t fClient = 0;		// xnudrmd's client id (0: not open)
	struct Slot { uint64_t id; IOMemoryMap *map; IOMemoryDescriptor *md; };
	Slot *fSlots = nullptr;
	unsigned fNSlots = 0;
	long simple(uint32_t op, uint64_t a0, uint64_t a1, xnudrm_hdr *reply);
	IOReturn doCall(IOExternalMethodArguments *a);
	IOReturn doMap(IOExternalMethodArguments *a);
	IOReturn doUnmap(IOExternalMethodArguments *a);
public:
	bool initWithTask(task_t owningTask, void *securityID, UInt32 type) override;
	IOReturn clientClose() override;
	void free() override;
	IOReturn externalMethod(uint32_t selector, IOExternalMethodArguments *arguments, IOExternalMethodDispatch *dispatch,
	    OSObject *target, void *reference) override;
};

OSDefineMetaClassAndStructors(XnuDrm, IOService)
OSDefineMetaClassAndStructors(AGXDeviceUserClient, IOUserClient)

static bool bridgeInit()
{
	if (gCallLock) return true;
	gIn = (uint8_t *)IOMallocData(sizeof(xnudrm_hdr) + XNUDRM_MAX_MSG);
	gOut = (uint8_t *)IOMallocData(sizeof(xnudrm_hdr) + XNUDRM_MAX_MSG);
	gCallLock = gIn && gOut ? IOLockAlloc() : nullptr;
	return gCallLock != nullptr;
}

bool XnuDrm::start(IOService *provider)
{
	if (!IOService::start(provider)) return false;
	if (!bridgeInit()) return false;
	setProperty("IOUserClientClass", "AGXDeviceUserClient");
	registerService();
	IOLog("XnuDrm: GPU bridge ready\n");
	return true;
}

bool AGXDeviceUserClient::initWithTask(task_t owningTask, void *securityID, UInt32 type)
{
	// only libxnudrm's opens (type 'XNDR'): Metal or anything else opening the accelerator gets no bridge client
	if (type != XNUDRM_CONNECT_TYPE) return false;
	if (!IOUserClient::initWithTask(owningTask, securityID, type)) return false;
	if (!bridgeInit()) return false;	// opened on the accelerator before XnuDrm started
	fTask = owningTask;
	xnudrm_hdr r;
	long n = simple(XNUDRM_OPEN, XNUDRM_VERSION, 0, &r);
	if (n < 0 || r.status) {
		IOLog("XnuDrm: xnudrmd did not open a client (%ld, %d)\n", n, n < 0 ? 0 : r.status);
		return false;
	}
	fClient = r.client;
	return true;
}

// A message without payload; the reply's header into *reply.
long AGXDeviceUserClient::simple(uint32_t op, uint64_t a0, uint64_t a1, xnudrm_hdr *reply)
{
	IOLockLock(gCallLock);
	xnudrm_hdr *h = (xnudrm_hdr *)gIn;
	*h = (xnudrm_hdr){ .magic = XNUDRM_MAGIC, .op = op, .client = fClient, .arg0 = a0, .arg1 = a1 };
	long n = hostcall(sizeof(*h));
	if (n >= 0) *reply = *(xnudrm_hdr *)gOut;
	IOLockUnlock(gCallLock);
	return n;
}

IOReturn AGXDeviceUserClient::doCall(IOExternalMethodArguments *a)
{
	user_addr_t in = a->scalarInput[0], out = a->scalarInput[2];
	uint64_t inlen = a->scalarInput[1], outmax = a->scalarInput[3];
	if (inlen < sizeof(xnudrm_hdr) || inlen > sizeof(xnudrm_hdr) + XNUDRM_MAX_MSG) return kIOReturnBadArgument;
	IOLockLock(gCallLock);
	IOReturn ret = kIOReturnSuccess;
	xnudrm_hdr *h = (xnudrm_hdr *)gIn;
	long n = -1;
	if (copyin(in, gIn, inlen)) { ret = kIOReturnVMError; goto out; }
	h->magic = XNUDRM_MAGIC;
	h->client = fClient;
	h->payload_len = (uint32_t)(inlen - sizeof(*h));
	if (h->op != XNUDRM_IOCTL && h->op != XNUDRM_FENCE_WAIT && h->op != XNUDRM_FENCE_CLOSE) { ret = kIOReturnUnsupported; goto out; }
	n = hostcall(inlen);
	if (n < 0) { IOLog("XnuDrm: host call failed (%ld)\n", n); ret = kIOReturnNotResponding; goto out; }
	if ((uint64_t)n > outmax) { ret = kIOReturnNoSpace; goto out; }
	if (copyout(gOut, out, n)) { ret = kIOReturnVMError; goto out; }
	a->scalarOutput[0] = n;
out:
	IOLockUnlock(gCallLock);
	return ret;
}

IOReturn AGXDeviceUserClient::doMap(IOExternalMethodArguments *a)
{
	uint64_t handle = a->scalarInput[0], size = a->scalarInput[1];
	IOLockLock(gCallLock);
	xnudrm_hdr *h = (xnudrm_hdr *)gIn;
	*h = (xnudrm_hdr){ .magic = XNUDRM_MAGIC, .op = XNUDRM_MAP, .client = fClient, .arg0 = handle, .arg1 = size };
	long n = hostcall(sizeof(*h));
	xnudrm_hdr *r = (xnudrm_hdr *)gOut;
	if (n < (long)sizeof(xnudrm_hdr) || r->status) {
		int rstatus = (n >= (long)sizeof(xnudrm_hdr)) ? r->status : 0;
		IOLockUnlock(gCallLock);
		IOLog("XnuDrm: map of handle %llu failed (%ld, %d)\n", handle, n, rstatus);
		return kIOReturnNoMemory;
	}
	uint64_t id = r->arg0;
	size_t avail = (size_t)n - sizeof(xnudrm_hdr);
	if (r->payload_len != avail || r->payload_len == 0 || (r->payload_len % sizeof(xnudrm_range)) != 0) {
		IOLockUnlock(gCallLock);
		xnudrm_hdr ur;
		simple(XNUDRM_UNMAP, id, 0, &ur);
		return kIOReturnBadArgument;
	}
	uint32_t nr = r->payload_len / sizeof(xnudrm_range);
	IOAddressRange *ranges = (IOAddressRange *)IOMallocData(nr * sizeof(IOAddressRange));
	if (!ranges) {
		IOLockUnlock(gCallLock);
		xnudrm_hdr ur;
		simple(XNUDRM_UNMAP, id, 0, &ur);
		return kIOReturnNoMemory;
	}
	const xnudrm_range *rg = (const xnudrm_range *)(gOut + sizeof(xnudrm_hdr));
	for (uint32_t i = 0; i < nr; i++) ranges[i] = (IOAddressRange){ rg[i].pa, rg[i].len };
	IOLockUnlock(gCallLock);

	// Linux's pages, outside of XNU's own memory: normal cacheable memory, as Linux maps them (the host keeps blob memory
	// coherent with the GPU)
	IOOptionBits opts = kIODirectionInOut;
	opts |= kIOMemoryTypePhysical64;
	opts |= kIOMemoryMapperNone;
	IOMemoryDescriptor *md = IOMemoryDescriptor::withAddressRanges(ranges, nr, opts, nullptr);
	IOFreeData(ranges, nr * sizeof(IOAddressRange));
	IOMemoryMap *map = md ? md->createMappingInTask(fTask, 0, kIOMapAnywhere | kIOMapCopybackCache) : nullptr;
	if (!map) {
		if (md) md->release();
		xnudrm_hdr ur;
		simple(XNUDRM_UNMAP, id, 0, &ur);
		return kIOReturnNoMemory;
	}
	unsigned i;
	for (i = 0; i < fNSlots && fSlots[i].map; i++) ;
	if (i == fNSlots) {
		unsigned nn = fNSlots ? fNSlots * 2 : 64;
		Slot *s = (Slot *)IOMallocZeroData(nn * sizeof(Slot));
		if (!s) {
			map->release();
			md->release();
			xnudrm_hdr ur;
			simple(XNUDRM_UNMAP, id, 0, &ur);
			return kIOReturnNoMemory;
		}
		if (fSlots) { memcpy(s, fSlots, fNSlots * sizeof(Slot)); IOFreeData(fSlots, fNSlots * sizeof(Slot)); }
		fSlots = s; fNSlots = nn;
	}
	fSlots[i] = (Slot){ id, map, md };
	a->scalarOutput[0] = map->getAddress();
	a->scalarOutput[1] = id;
	return kIOReturnSuccess;
}

IOReturn AGXDeviceUserClient::doUnmap(IOExternalMethodArguments *a)
{
	uint64_t id = a->scalarInput[0];
	for (unsigned i = 0; i < fNSlots; i++) {
		if (fSlots[i].map && fSlots[i].id == id) {
			fSlots[i].map->release();	// unmaps from the task before Linux lets go of the pages
			fSlots[i].md->release();
			fSlots[i] = (Slot){};
			xnudrm_hdr r;
			simple(XNUDRM_UNMAP, id, 0, &r);
			return kIOReturnSuccess;
		}
	}
	return kIOReturnBadArgument;
}

IOReturn AGXDeviceUserClient::externalMethod(uint32_t selector, IOExternalMethodArguments *a, IOExternalMethodDispatch *,
    OSObject *, void *)
{
	switch (selector) {
	case 0: if (a->scalarInputCount != 4 || a->scalarOutputCount != 1) return kIOReturnBadArgument; return doCall(a);
	case 1: if (a->scalarInputCount != 2 || a->scalarOutputCount != 2) return kIOReturnBadArgument; return doMap(a);
	case 2: if (a->scalarInputCount != 1) return kIOReturnBadArgument; return doUnmap(a);
	}
	return kIOReturnUnsupported;
}

IOReturn AGXDeviceUserClient::clientClose()
{
	for (unsigned i = 0; i < fNSlots; i++) {
		if (fSlots[i].map) { fSlots[i].map->release(); fSlots[i].md->release(); fSlots[i] = (Slot){}; }
	}
	if (fClient) {
		xnudrm_hdr r;
		simple(XNUDRM_CLOSE, 0, 0, &r);	// xnudrmd drops the client's mappings, fences and DRM file
		fClient = 0;
	}
	terminate();
	return kIOReturnSuccess;
}

void AGXDeviceUserClient::free()
{
	if (fSlots) IOFreeData(fSlots, fNSlots * sizeof(Slot));
	IOUserClient::free();
}

extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *) { return KERN_SUCCESS; }
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

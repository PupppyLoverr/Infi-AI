// SPDX-License-Identifier: APSL-2.0
// XnuDisk: the hosted macOS's internal disk, a block storage device whose blocks are a file on the Linux side.
//
// The file is a whole disk as macOS makes one: a GUID partition table and an APFS container (System and Data volumes),
// built on a macOS host with Apple's own tools (scripts/mk-macvm-disk.sh). IOStorageFamily's IOBlockStorageDriver takes
// this device, publishes its media, the GUID partition scheme finds the container partition and APFS mounts its volumes,
// as on a Mac's internal SSD. I/O goes through the host: xnu.ko disk=, LINUX64_CALL_DISK_RW and _SIZE in
// bsd/kern/linux64_host.c, done synchronously through a bounce buffer (the host reads and writes the file in Linux context).
// IOBlockStorageDevice.h declares some of its virtual methods (doLockUnlockMedia, reportLockability,
// reportPollRequirements, doDiscard) only under TARGET_OS_OSX; the collection's IOStorageFamily is the macOS one, so
// without it this class's vtable is three slots short of the family's and the family's calls land on the wrong methods
// (a PAC failure at the first, reportRemovability, from IOBlockStorageDriver::handleStart).
#include <TargetConditionals.h>
#undef TARGET_OS_OSX
#define TARGET_OS_OSX 1
#include <IOKit/IOLib.h>
#include <IOKit/IOLocks.h>
#include <IOKit/IOMemoryDescriptor.h>
#include <IOKit/storage/IOBlockStorageDevice.h>
#include <IOKit/storage/IOStorageProtocolCharacteristics.h>
#include <IOKit/storage/IOStorageDeviceCharacteristics.h>
#include <kern/thread.h>

extern "C" uint64_t linux64_disk_size(void);
extern "C" long linux64_disk_rw(int write, void *buffer, unsigned long length, unsigned long offset);

namespace {
constexpr UInt64 kBlockSize = 512;          // the partition table's LBA size as the host tools wrote it
constexpr IOByteCount kBounce = 1024 * 1024; // the largest piece one upcall moves
}

class XnuDisk : public IOBlockStorageDevice {
    OSDeclareDefaultStructors(XnuDisk)
    UInt64 fBlocks = 0, fBlockSize = kBlockSize;
    IOLock *fLock = nullptr;
    uint8_t *fBounce = nullptr;
    char fVendor[8] = "Apple", fProduct[24] = "xnulinux Disk", fRevision[8] = "1.0", fInfo[4] = "";

public:
    bool start(IOService *provider) override;
    static void publishAfterBSD(void *self, wait_result_t);
    void free() override;
    IOReturn doEjectMedia() override { return kIOReturnUnsupported; }
    IOReturn doFormatMedia(UInt64) override { return kIOReturnUnsupported; }
    UInt32 doGetFormatCapacities(UInt64 *capacities, UInt32 max) const override
    {
        if (capacities && max)
            capacities[0] = fBlocks * fBlockSize;
        return 1;
    }
    char *getVendorString() override { return fVendor; }
    char *getProductString() override { return fProduct; }
    char *getRevisionString() override { return fRevision; }
    char *getAdditionalDeviceInfoString() override { return fInfo; }
    IOReturn reportBlockSize(UInt64 *size) override { *size = fBlockSize; return kIOReturnSuccess; }
    IOReturn reportEjectability(bool *ejectable) override { *ejectable = false; return kIOReturnSuccess; }
    IOReturn reportMaxValidBlock(UInt64 *block) override { *block = fBlocks - 1; return kIOReturnSuccess; }
    IOReturn reportMediaState(bool *present, bool *changed) override
    {
        *present = true;
        if (changed)
            *changed = false;
        return kIOReturnSuccess;
    }
    IOReturn reportRemovability(bool *removable) override { *removable = false; return kIOReturnSuccess; }
    IOReturn reportWriteProtection(bool *protect) override { *protect = false; return kIOReturnSuccess; }
    IOReturn getWriteCacheState(bool *enabled) override { *enabled = false; return kIOReturnSuccess; }
    IOReturn setWriteCacheState(bool) override { return kIOReturnSuccess; }
    IOReturn doSynchronize(UInt64, UInt64, IOStorageSynchronizeOptions) override { return kIOReturnSuccess; }
    IOReturn doAsyncReadWrite(IOMemoryDescriptor *buffer, UInt64 block, UInt64 nblks, IOStorageAttributes *attributes,
                              IOStorageCompletion *completion) override;
};

OSDefineMetaClassAndStructors(XnuDisk, IOBlockStorageDevice)

bool XnuDisk::start(IOService *provider)
{
    UInt64 bytes = linux64_disk_size();
    if (bytes < 2 * 4096 || !IOBlockStorageDevice::start(provider))
        return false;
    fLock = IOLockAlloc();
    fBounce = (uint8_t *)IOMallocData(kBounce);
    if (!fLock || !fBounce)
        return false;
    // ... or 4096, when the GPT header (LBA 1) is at byte 4096 and not at 512
    if (linux64_disk_rw(0, fBounce, 8, 4096) == 8 && !memcmp(fBounce, "EFI PART", 8) &&
        !(linux64_disk_rw(0, fBounce, 8, 512) == 8 && !memcmp(fBounce, "EFI PART", 8)))
        fBlockSize = 4096;
    fBlocks = bytes / fBlockSize;
    // an internal disk, as the Mac's own: APFS and the system treat the container on it as the boot disk's
    OSDictionary *protocol = OSDictionary::withCapacity(2);
    OSDictionary *device = OSDictionary::withCapacity(2);
    if (protocol && device) {
        OSString *virt = OSString::withCString(kIOPropertyPhysicalInterconnectTypeVirtual);
        OSString *internal = OSString::withCString(kIOPropertyInternalKey);
        OSString *product = OSString::withCString(fProduct);
        OSString *vendor = OSString::withCString(fVendor);
        protocol->setObject(kIOPropertyPhysicalInterconnectTypeKey, virt);
        protocol->setObject(kIOPropertyPhysicalInterconnectLocationKey, internal);
        device->setObject(kIOPropertyProductNameKey, product);
        device->setObject(kIOPropertyVendorNameKey, vendor);
        setProperty(kIOPropertyProtocolCharacteristicsKey, protocol);
        setProperty(kIOPropertyDeviceCharacteristicsKey, device);
        OSSafeReleaseNULL(virt);
        OSSafeReleaseNULL(internal);
        OSSafeReleaseNULL(product);
        OSSafeReleaseNULL(vendor);
    }
    OSSafeReleaseNULL(protocol);
    OSSafeReleaseNULL(device);
    /*
     * The disk is published once the IOBSD resource is (bsd_init's IOKitBSDInit publishes it, then waits for the root
     * disk). Published earlier, it can deadlock the boot: APFS starts the first container's volumes holding its lock and
     * waits there for IOBSD on IOResources, while IOResources' own matching (re-run when IOBSD is published) probes
     * an APFS personality that needs that lock; the main container is never probed and bsd_init waits for its root
     * volume forever (often, on one CPU). A Mac's internal disk sits under its PCIe controller and
     * appears in its own time; ours hangs off IOResources and appears at once.
     */
    thread_t thread;
    retain();
    if (kernel_thread_start(&XnuDisk::publishAfterBSD, this, &thread) == KERN_SUCCESS) {
        thread_deallocate(thread);
    } else {
        release();
        registerService();
    }
    IOLog("XnuDisk: %llu blocks of %llu bytes (%llu MB)\n", fBlocks, fBlockSize, bytes >> 20);
    return true;
}

void XnuDisk::publishAfterBSD(void *arg, wait_result_t)
{
    XnuDisk *self = (XnuDisk *)arg;
    OSDictionary *match = IOService::resourceMatching("IOBSD");
    IOService *bsd = match ? IOService::waitForMatchingService(match) : nullptr;
    OSSafeReleaseNULL(match);
    OSSafeReleaseNULL(bsd);
    self->registerService();
    IOLog("XnuDisk: published after IOBSD\n");
    self->release();
    thread_terminate(current_thread());
}

void XnuDisk::free()
{
    if (fBounce)
        IOFreeData(fBounce, kBounce);
    if (fLock)
        IOLockFree(fLock);
    IOBlockStorageDevice::free();
}

IOReturn XnuDisk::doAsyncReadWrite(IOMemoryDescriptor *buffer, UInt64 block, UInt64 nblks, IOStorageAttributes *,
                                   IOStorageCompletion *completion)
{
    if (!buffer || block + nblks > fBlocks)
        return kIOReturnBadArgument;
    bool write = buffer->getDirection() == kIODirectionOut;
    IOByteCount total = nblks * fBlockSize, done = 0;
    IOReturn status = buffer->prepare();
    if (status == kIOReturnSuccess) {
        IOLockLock(fLock);
        while (done < total) {
            IOByteCount piece = total - done < kBounce ? total - done : kBounce;
            if (write && buffer->readBytes(done, fBounce, piece) != piece) {
                status = kIOReturnIOError;
                break;
            }
            long moved = linux64_disk_rw(write, fBounce, piece, block * fBlockSize + done);
            if (moved != (long)piece) {
                IOLog("XnuDisk: %s of %llu bytes at block %llu failed (%ld)\n", write ? "write" : "read", (UInt64)piece,
                      block + done / fBlockSize, moved);
                status = kIOReturnIOError;
                break;
            }
            if (!write && buffer->writeBytes(done, fBounce, piece) != piece) {
                status = kIOReturnIOError;
                break;
            }
            done += piece;
        }
        IOLockUnlock(fLock);
        buffer->complete();
    }
    IOStorage::complete(completion, status, status == kIOReturnSuccess ? total : done);
    return kIOReturnSuccess;
}

// Module start/stop for the collection's kext entry; the driver starts through IOKit matching.
#include <mach/kmod.h>
extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *) { return KERN_SUCCESS; }
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

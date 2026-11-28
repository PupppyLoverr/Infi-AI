// SPDX-License-Identifier: BSD-2-Clause-Patent
// XnuEthernet: en0 of the hosted macOS, an IOEthernetController whose frames go through the host. It follows DroidVMEthernet of
// Droid-VM/macOS-DroidVM (overlay/kext/DroidVMEthernet.cpp, BSD-2-Clause-Patent) without its virtio-net: the transport is the
// hosted kernel's (bsd/kern/linux64_host.c, LINUX64_CALL_NET_SEND/RECV to the Linux netdev xnu0, where Linux routes, NATs and answers DHCP).
//
// Why a driver and not only the kernel's own ifnet: IOTimeSyncClockManager takes the TimeSync clock identifier from a built-in
// IOEthernetInterface's MAC, once, at its start; without one it never registers, clocksyncd waits for it and WindowServer
// (QuartzCore's TimeSync clock) waits on clocksyncd. The collection gates IOTimeSyncRootService on the resource published here.
#include <IOKit/IOLib.h>
#include <IOKit/IOLocks.h>
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-anon-enum-enum-conversion"   // the family's headers mix anonymous enums
#define __PRIVATE_SPI__   // the shipped family uses IONetworkController's reserved slots 2-5 (outputStart ... networkInterfaceNotification)
#include <IOKit/network/IOEthernetController.h>
#include <IOKit/network/IOEthernetInterface.h>
#include <IOKit/network/IONetworkMedium.h>
#include <IOKit/network/IOOutputQueue.h>   // kIOReturnOutputSuccess, kIOReturnOutputDropped
#pragma clang diagnostic pop
#include <kern/thread.h>
#include <net/ethernet.h>

// bsd/kern/linux64_host.c (the xnulinux kernel patch): one frame to the host, the next frame from it (bytes, 0 when none waits).
extern "C" long linux64_net_send(const void *frame, unsigned long length);
extern "C" long linux64_net_recv(void *buffer, unsigned long size);

namespace {
constexpr unsigned frameBytes = 1514;       // a full ethernet frame without the FCS
constexpr unsigned fastPollMs = 1, idlePollMs = 10, fastPollTicks = 1000;   // 1 ms polls for a second after any traffic
constexpr UInt64 linkSpeed = 1000 * 1000000ULL;
}

class XnuEthernet : public IOEthernetController {
    OSDeclareDefaultStructors(XnuEthernet)

    IOEthernetInterface *fInterface = nullptr;
    IONetworkStats *fStats = nullptr;   // the interface's counts of packets and errors sent: the driver keeps them (netstat's Opkts, Oerrs)
    IOEthernetAddress fAddress = {{0x02, 0x58, 0x4e, 0x55, 0x00, 0x01}};   // locally administered, "XNU" (the kernel's en0 had it)
    IOLock *fTxLock = nullptr;
    uint8_t fTx[frameBytes + 64], fRx[frameBytes + 64];
    volatile bool fEnabled = false, fActive = false;
    UInt64 fRxPackets = 0, fTxPackets = 0, fRxDrops = 0, fTxDrops = 0;

    void pollReceive();
    static void pollThread(void *self, wait_result_t);

public:
    bool start(IOService *provider) override;
    void stop(IOService *provider) override;
    void free() override;
    IOReturn getHardwareAddress(IOEthernetAddress *address) override;
    IOReturn enable(IONetworkInterface *interface) override;
    IOReturn disable(IONetworkInterface *interface) override;
    // IOBSD is up (our IOResourceMatch); the family's wait for it from start() needs IOResources matched, which it is not while
    // IOResources' own matching starts us (DroidVMEthernet)
    UInt32 getFeatures() const override { return kIONetworkFeatureNoBSDWait; }
    UInt32 outputPacket(mbuf_t packet, void *param) override;
    IOReturn setPromiscuousMode(bool active) override { return kIOReturnSuccess; }
    IOReturn setMulticastMode(bool active) override { return kIOReturnSuccess; }
    IOReturn setMulticastList(IOEthernetAddress *addresses, UInt32 count) override { return kIOReturnSuccess; }
};

OSDefineMetaClassAndStructors(XnuEthernet, IOEthernetController)

bool XnuEthernet::start(IOService *provider)
{
    if (!IOEthernetController::start(provider))
        return false;
    fTxLock = IOLockAlloc();
    if (!fTxLock)
        return false;
    OSDictionary *media = OSDictionary::withCapacity(1);
    IONetworkMedium *medium = IONetworkMedium::medium(IOMediumType(kIOMediumEthernetAuto) | IOMediumType(kIOMediumOptionFullDuplex), linkSpeed);
    if (!media || !medium || !IONetworkMedium::addMedium(media, medium) || !publishMediumDictionary(media)) {
        OSSafeReleaseNULL(media);
        OSSafeReleaseNULL(medium);
        IOLog("XnuEthernet: cannot publish the medium\n");
        return false;
    }
    media->release();
    setSelectedMedium(medium);
    setLinkStatus(kIONetworkLinkValid | kIONetworkLinkActive, medium, linkSpeed);
    medium->release();
    if (!attachInterface(reinterpret_cast<IONetworkInterface **>(&fInterface), false)) {
        IOLog("XnuEthernet: cannot attach the interface\n");
        return false;
    }
    if (IONetworkData *data = fInterface->getNetworkData(kIONetworkStatsKey))
        fStats = static_cast<IONetworkStats *>(const_cast<void *>(data->getBuffer()));
    // IOTimeSyncClockManager takes the clock identifier from the built-in interfaces only and looks once, at its start
    fInterface->setProperty(kIOBuiltin, true);
    fInterface->registerService(kIOServiceSynchronous);
    publishResource("XnuEthernet");
    thread_t thread;
    if (kernel_thread_start(pollThread, this, &thread) == KERN_SUCCESS)
        thread_deallocate(thread);
    const uint8_t *m = fAddress.bytes;
    IOLog("XnuEthernet: en0 through the host's xnu0, MAC %02x:%02x:%02x:%02x:%02x:%02x\n", m[0], m[1], m[2], m[3], m[4], m[5]);
    return true;
}

void XnuEthernet::stop(IOService *provider)
{
    if (fInterface) {
        detachInterface(fInterface);
        OSSafeReleaseNULL(fInterface);
    }
    IOEthernetController::stop(provider);
}

void XnuEthernet::free()
{
    if (fTxLock)
        IOLockFree(fTxLock);
    fTxLock = nullptr;
    IOEthernetController::free();
}

IOReturn XnuEthernet::getHardwareAddress(IOEthernetAddress *address)
{
    *address = fAddress;
    return kIOReturnSuccess;
}

IOReturn XnuEthernet::enable(IONetworkInterface *interface)
{
    fEnabled = true;
    return kIOReturnSuccess;
}

IOReturn XnuEthernet::disable(IONetworkInterface *interface)
{
    fEnabled = false;
    return kIOReturnSuccess;
}

// No output queue: the interface calls in from the stack's threads, so the one transmit buffer is taken under a lock.
// The return value goes to the stack as if_output's errno (IONetworkInterface::if_output): a frame dropped here is lost like one
// dropped on the wire, 0; an output queue's kIOReturnOutputDropped (1) would be EPERM to the sender.
UInt32 XnuEthernet::outputPacket(mbuf_t packet, void *param)
{
    size_t length = mbuf_pkthdr_len(packet);
    if (!fEnabled || length < ETHER_HDR_LEN || length > frameBytes) {
        ++fTxDrops;
        freePacket(packet);
        return kIOReturnOutputSuccess;
    }
    IOLockLock(fTxLock);
    long sent = mbuf_copydata(packet, 0, length, fTx) == 0 ? linux64_net_send(fTx, length) : 0;
    if (fStats)   // with no output queue the interface takes the output counts from here (IONetworkInterface::if_output)
        ++(sent > 0 ? fStats->outputPackets : fStats->outputErrors);
    IOLockUnlock(fTxLock);
    freePacket(packet);
    if (sent <= 0) {
        ++fTxDrops;
        return kIOReturnOutputSuccess;
    }
    ++fTxPackets;
    fActive = true;
    return kIOReturnOutputSuccess;
}

// Takes every frame the host has queued and hands it to the interface. One thread only, so the interface's inputPacket calls are
// serialized as the family requires. The host's xnu0 delivers only what Linux routes to this guest: no filter here.
void XnuEthernet::pollReceive()
{
    for (unsigned n = 0; n < 256; n++) {
        long length = linux64_net_recv(fRx, sizeof(fRx));
        if (length < ETHER_HDR_LEN)
            return;
        mbuf_t packet = nullptr;
        int copied = -1;
        if (fEnabled && fInterface && length <= frameBytes && (packet = allocatePacket(static_cast<UInt32>(length))) &&
            (copied = mbuf_copyback(packet, 0, static_cast<size_t>(length), fRx, MBUF_DONTWAIT)) == 0) {
            fInterface->inputPacket(packet, static_cast<UInt32>(length));
            ++fRxPackets;
        } else {
            if (packet)
                freePacket(packet);
            if (++fRxDrops <= 3 && fEnabled)   // frames before the interface is enabled (boot) are dropped as expected
                IOLog("XnuEthernet: frame of %ld bytes dropped (packet %p, copyback %d)\n", length, packet, copied);
        }
        fActive = true;
    }
}

// 1 ms polls for a second after traffic, 10 ms otherwise (as DroidVMEthernet).
void XnuEthernet::pollThread(void *self, wait_result_t)
{
    XnuEthernet *ethernet = static_cast<XnuEthernet *>(self);
    for (unsigned quiet = fastPollTicks;;) {
        ethernet->pollReceive();
        if (ethernet->fActive) {
            ethernet->fActive = false;
            quiet = 0;
        } else if (quiet < fastPollTicks) {
            ++quiet;
        }
        IOSleep(quiet < fastPollTicks ? fastPollMs : idlePollMs);
    }
}

// Module start/stop for the collection's kext entry; the driver starts through IOKit matching.
#include <mach/kmod.h>
extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *) { return KERN_SUCCESS; }
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

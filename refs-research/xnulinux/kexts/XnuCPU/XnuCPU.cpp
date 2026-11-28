// SPDX-License-Identifier: APSL-2.0
// XnuCPU: the hosted system's CPUs, one IOCPU per /cpus/cpuN node xnu.ko describes (compatible "xnulinux,cpu").
//
// On a Mac the platform's CPU driver registers each CPU with the kernel (ml_processor_register) and starts and signals
// them through its own hardware; while hosted, the CPUs are Linux's: XNU's machine layer starts a CPU through the host
// (cpu_start, LINUX64_CALL_CPU_START: xnu.ko runs it on a Linux kernel thread of that CPU) and signals one by setting
// its pending IPI and asking the host to make it look (linux64_cpu_signal). This driver only registers the CPUs (XNU 12377
// boots each one inside ml_processor_register) and routes IOCPU's signalCPU there. Its structure follows the ArmCpu driver of
// DroidVM (BSD-2-Clause-Patent), without the GIC.
#include <IOKit/IOCPU.h>
#include <IOKit/IOLib.h>
#include <IOKit/IOLocks.h>
#include <libkern/c++/OSData.h>
#include <libkern/c++/OSSymbol.h>
#include <machine/machine_routines.h>

extern "C" void linux64_cpu_signal(unsigned int cpu);

class XnuCPU;

namespace {
// Reported idle latency: ml_delay_should_spin() blocks every IODelay when this is 0 (never reported), turning each short
// delay into a timer; with a short latency short delays spin and long ones still block (as ArmCpu found).
constexpr uint64_t kIdleLatencyNs = 5000;
constexpr unsigned kMaxCPUs = 64;

// The boot CPU registers first (registering boots a CPU, and the others send it IPIs from then on, as AppleARMSMP's
// order is); a secondary matched before it waits here and is registered by the boot CPU's start
IOLock *gLock;
bool gBootRegistered;
XnuCPU *gWaiting[kMaxCPUs];
unsigned gWaitingCount;
}

class XnuCPU : public IOCPU {
    OSDeclareDefaultStructors(XnuCPU)
    uint32_t fPhysical = 0, fLogical = 0;
    const OSSymbol *fName = nullptr;
    bool registerProcessor();

public:
    bool start(IOService *provider) override;
    void initCPU(bool boot) override;
    void quiesceCPU() override {}
    void haltCPU() override { setCPUState(kIOCPUStateStopped); }
    kern_return_t startCPU(vm_offset_t, vm_offset_t) override { return KERN_SUCCESS; }	// cpu_start asks the host
    void signalCPU(IOCPU *target) override { linux64_cpu_signal(target->getCPUNumber()); }
    const OSSymbol *getCPUName() override { return fName; }
};

OSDefineMetaClassAndStructors(XnuCPU, IOCPU)

bool XnuCPU::start(IOService *provider)
{
    OSData *reg = OSDynamicCast(OSData, provider->getProperty("reg"));
    if (reg == nullptr || reg->getLength() < sizeof(fPhysical)) {
        return false;
    }
    memcpy(&fPhysical, reg->getBytesNoCopy(), sizeof(fPhysical));
    fLogical = static_cast<uint32_t>(ml_get_cpu_number(fPhysical));
    bool boot = fLogical == static_cast<uint32_t>(ml_get_boot_cpu_number());
    fName = OSSymbol::withCStringNoCopy(boot ? "xnulinux boot CPU" : "xnulinux CPU");
    if (fName == nullptr || !IOCPU::start(provider)) {
        return false;
    }
    setCPUNumber(fLogical);
    if (boot) {
        if (!registerProcessor()) {
            return false;
        }
        IOLockLock(gLock);
        gBootRegistered = true;
        unsigned n = gWaitingCount;
        gWaitingCount = 0;
        IOLockUnlock(gLock);
        for (unsigned i = 0; i < n; i++) {
            gWaiting[i]->registerProcessor();
            gWaiting[i]->release();
        }
        return true;
    }
    IOLockLock(gLock);
    if (!gBootRegistered && gWaitingCount < kMaxCPUs) {
        retain();
        gWaiting[gWaitingCount++] = this;
        IOLockUnlock(gLock);
        return true;
    }
    IOLockUnlock(gLock);
    return registerProcessor();
}

// Register this CPU with the kernel; XNU 12377's ml_processor_register boots it too (processor_boot: cpu_start, which
// asks the host)
bool XnuCPU::registerProcessor()
{
    bool boot = fLogical == static_cast<uint32_t>(ml_get_boot_cpu_number());
    ml_processor_info_t info = {};
    info.cpu_id = this;
    info.phys_id = fPhysical;
    info.log_id = fLogical;
    info.cluster_id = static_cast<uint32_t>(ml_get_cluster_number(fPhysical));
    info.cluster_type = ml_get_boot_cluster_type();
    info.powergate_latency = kIdleLatencyNs;
    ipi_handler_t ipi = nullptr;
    perfmon_interrupt_handler_func pmi = nullptr;
    if (ml_processor_register(&info, &machProcessor, &ipi, &pmi) != KERN_SUCCESS) {
        IOLog("XnuCPU: CPU %u (MPIDR %#x) registration failed\n", fLogical, fPhysical);
        return false;
    }
    ipi_handler = ipi;	// the machine layer calls cpu_signal_handler itself (linux64_ipi_take)
    registerService();
    IOLog("XnuCPU: CPU %u (MPIDR %#x) registered%s\n", fLogical, fPhysical, boot ? ", boot CPU" : "");
    return true;
}

void XnuCPU::initCPU(bool boot)
{
    setCPUState(kIOCPUStateRunning);
    // XNU clears a CPU's SIGPdisabled when it takes its first IPI (processor_up): send it one, as ArmCpu does
    signalCPU(this);
    (void)boot;
}

// Module start/stop for the collection's kext entry; the driver starts through IOKit matching.
#include <mach/kmod.h>
extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *)
{
    gLock = IOLockAlloc();
    return gLock ? KERN_SUCCESS : KERN_FAILURE;
}
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

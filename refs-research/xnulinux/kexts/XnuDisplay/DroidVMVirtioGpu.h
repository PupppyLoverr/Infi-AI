// SPDX-License-Identifier: BSD-2-Clause-Patent
// Stands in for the DroidVM board's virtio-gpu transport (DroidVMVirtioGpu.h of Droid-VM/UniXNU-droidvm) that XnuDisplay.cpp
// (DroidVMDisplay) is written against. The hosted kernel has no virtio-gpu: start() fails, so the display stays in stage 1 (the CPU
// copy into the boot framebuffer), and nothing here is reached otherwise. The 3D submission counters say every submission is done.
#pragma once
#include <IOKit/IOReturn.h>
#include <stdint.h>

namespace DroidVMVirtioGpu {
struct Header {uint32_t type, flags; uint64_t fence; uint32_t context; uint8_t ring, padding[3];};
struct Entry {uint64_t address; uint32_t length, padding;};
static constexpr uint32_t MaxCommand = 8 * 1024 * sizeof(Entry) + 64;
static inline bool start() {return false;}
static inline bool ready() {return false;}
static inline uint64_t features() {return 0;}
static inline IOReturn command(const void *, uint32_t, const void *, uint32_t, void *, uint32_t) {return kIOReturnNoDevice;}
static inline IOReturn post(const void *, uint32_t, const void *, uint32_t, uint32_t, bool, uint32_t *) {return kIOReturnNoDevice;}
static inline IOReturn wait(uint32_t, uint64_t, void *, uint32_t) {return kIOReturnNoDevice;}
static inline IOReturn cursor(uint32_t, int32_t, int32_t, uint32_t, uint32_t, bool) {return kIOReturnNoDevice;}
static inline void unref(uint32_t) {}
static inline uint32_t resourceId() {return 0;}
static inline void selftest3D() {}
}

extern "C" {
volatile uint64_t droidvm_virtio_gpu_commands;
uint64_t droidvm_virtgpu_submitted(void) {return 0;}
bool droidvm_virtgpu_finished(uint64_t) {return true;}
bool droidvm_virtgpu_wait_finished(uint64_t, uint64_t) {return true;}
}

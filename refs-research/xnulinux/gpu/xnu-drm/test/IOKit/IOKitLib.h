/* SPDX-License-Identifier: MIT */
/* The little of IOKit that libxnudrm uses, for fence.c: the host call ends in the test. */
#ifndef XNUDRM_TEST_IOKITLIB_H
#define XNUDRM_TEST_IOKITLIB_H
#include <stddef.h>
#include <stdint.h>
#ifndef __APPLE__
#include <sys/sysmacros.h>
#endif
typedef uint32_t io_connect_t, io_service_t;
#define kern_return_t int
#undef KERN_SUCCESS
#define KERN_SUCCESS 0
#define kIOMainPortDefault 0
#define IOServiceMatching(name) (name)
#define IOServiceGetMatchingService(port, match) ((void)(match), 1u)
#undef mach_task_self
#define mach_task_self() 0
int IOServiceOpen(io_service_t sv, int task, uint32_t type, io_connect_t *c);
int IOServiceClose(io_connect_t c);
#define IOObjectRelease(o) ((void)(o))
int IOConnectCallScalarMethod(io_connect_t c, uint32_t sel, const uint64_t *in, uint32_t nin, uint64_t *out, uint32_t *nout);
#endif

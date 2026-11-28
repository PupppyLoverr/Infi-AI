// SPDX-License-Identifier: APSL-2.0
/* For Mbed TLS in the kernel (-idirafter): oid.c includes <stdio.h> for snprintf, which the config replaces. */
#ifndef XNULINUX_CC_COMPAT_STDIO_H
#define XNULINUX_CC_COMPAT_STDIO_H
#include <stddef.h>
#endif

// SPDX-License-Identifier: APSL-2.0
// The kernel has no <stdio.h>; PureDarwin's AMFI stub includes it for printf and snprintf only.
#ifndef XNULINUX_KEXT_STDIO_H
#define XNULINUX_KEXT_STDIO_H
#include <stddef.h>
int printf(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
int snprintf(char *buf, size_t len, const char *fmt, ...) __attribute__((format(printf, 3, 4)));
#endif

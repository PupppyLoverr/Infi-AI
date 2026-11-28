// SPDX-License-Identifier: APSL-2.0
/* Tiny freestanding helpers so xnu.ko needs nothing from Linux beyond <linux/fki.h>. */
#pragma once
static inline void *x_memcpy(void *d, const void *s, unsigned long n) { char *a = d; const char *b = s; while (n--) *a++ = *b++; return d; }
static inline int x_strncmp(const char *a, const char *b, unsigned long n) { for (; n; n--, a++, b++) { if (*a != *b) return (unsigned char)*a - (unsigned char)*b; if (!*a) return 0; } return 0; }
static inline int x_strcmp(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return (unsigned char)*a - (unsigned char)*b; }
static inline unsigned long x_strlen(const char *s) { const char *p = s; while (*p) p++; return p - s; }

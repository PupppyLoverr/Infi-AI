// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/*
 * Mbed TLS (scripts/free-kc/build.sh fetches it at a pinned release) as this kext uses it: the primitives behind the
 * corecrypto interface, nothing else. No I/O, no time, no threads; memory from the kext (cc_platform.c).
 */
#ifndef MBEDTLS_CONFIG_XNULINUX_H
#define MBEDTLS_CONFIG_XNULINUX_H

#define MBEDTLS_PLATFORM_C
#define MBEDTLS_PLATFORM_MEMORY
#define MBEDTLS_PLATFORM_NO_STD_FUNCTIONS
#define MBEDTLS_PLATFORM_CALLOC_MACRO xnulinux_cc_calloc
#define MBEDTLS_PLATFORM_FREE_MACRO   xnulinux_cc_free
#define MBEDTLS_PLATFORM_SNPRINTF_MACRO xnulinux_cc_snprintf	/* OIDs as text: never needed here */
#define MBEDTLS_DEPRECATED_REMOVED

/* digests */
#define MBEDTLS_MD_C
#define MBEDTLS_MD5_C
#define MBEDTLS_SHA1_C
#define MBEDTLS_SHA224_C
#define MBEDTLS_SHA256_C
#define MBEDTLS_SHA384_C
#define MBEDTLS_SHA512_C

/* ciphers and modes */
#define MBEDTLS_AES_C
#define MBEDTLS_CIPHER_MODE_CBC
#define MBEDTLS_CIPHER_MODE_CTR
#define MBEDTLS_GCM_C
#define MBEDTLS_DES_C
#define MBEDTLS_CHACHA20_C
#define MBEDTLS_POLY1305_C
#define MBEDTLS_CHACHAPOLY_C

/* RSA PKCS #1 v1.5 signature verification */
#define MBEDTLS_BIGNUM_C
#define MBEDTLS_RSA_C
#define MBEDTLS_PKCS1_V15
#define MBEDTLS_OID_C
#define MBEDTLS_NO_UDBL_DIVISION	/* no 128-bit division in the kernel (__udivti3) */
#define MBEDTLS_ASN1_PARSE_C

/* arm64: AES and SHA-256 with the CPU's crypto instructions only (every host xnulinux runs on has them) */
#if defined(__aarch64__)
#define MBEDTLS_AESCE_C
#define MBEDTLS_AES_USE_HARDWARE_ONLY
#define MBEDTLS_SHA256_USE_ARMV8_A_CRYPTO_ONLY
#endif

#include <stddef.h>
void *xnulinux_cc_calloc(size_t n, size_t size);
void xnulinux_cc_free(void *p);
int xnulinux_cc_snprintf(char *s, size_t n, const char *format, ...);

#endif /* MBEDTLS_CONFIG_XNULINUX_H */

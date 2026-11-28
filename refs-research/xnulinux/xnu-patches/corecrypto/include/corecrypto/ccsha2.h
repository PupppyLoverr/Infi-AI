// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccsha2.h: the SHA-2 family (FIPS 180-4). */
#ifndef _CORECRYPTO_CCSHA2_H_
#define _CORECRYPTO_CCSHA2_H_

#include <corecrypto/ccdigest.h>

const struct ccdigest_info *ccsha224_di(void);
const struct ccdigest_info *ccsha256_di(void);
const struct ccdigest_info *ccsha384_di(void);
const struct ccdigest_info *ccsha512_di(void);

#define CCSHA224_OUTPUT_SIZE 28
#define CCSHA256_BLOCK_SIZE  64
#define CCSHA256_OUTPUT_SIZE 32
#define CCSHA256_STATE_SIZE  32
#define CCSHA384_OUTPUT_SIZE 48
#define CCSHA512_BLOCK_SIZE  128
#define CCSHA512_OUTPUT_SIZE 64
#define CCSHA512_STATE_SIZE  64

#define ccoid_sha224     ((unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x04")
#define ccoid_sha224_len 11
#define ccoid_sha256     ((unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x01")
#define ccoid_sha256_len 11
#define ccoid_sha384     ((unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x02")
#define ccoid_sha384_len 11
#define ccoid_sha512     ((unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x03")
#define ccoid_sha512_len 11

#endif /* _CORECRYPTO_CCSHA2_H_ */

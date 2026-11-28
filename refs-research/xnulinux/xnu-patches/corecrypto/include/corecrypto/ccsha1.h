// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccsha1.h: SHA-1 (FIPS 180-4). */
#ifndef _CORECRYPTO_CCSHA1_H_
#define _CORECRYPTO_CCSHA1_H_

#include <corecrypto/ccdigest.h>

#define CCSHA1_BLOCK_SIZE  64
#define CCSHA1_OUTPUT_SIZE 20
#define CCSHA1_STATE_SIZE  20

const struct ccdigest_info *ccsha1_di(void);

#define ccoid_sha1     ((unsigned char *)"\x06\x05\x2b\x0e\x03\x02\x1a")
#define ccoid_sha1_len 7

#endif /* _CORECRYPTO_CCSHA1_H_ */

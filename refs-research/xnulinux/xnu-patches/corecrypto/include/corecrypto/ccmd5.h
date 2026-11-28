// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccmd5.h: MD5 (RFC 1321), for the protocols that still name it. */
#ifndef _CORECRYPTO_CCMD5_H_
#define _CORECRYPTO_CCMD5_H_

#include <corecrypto/ccdigest.h>

#define CCMD5_BLOCK_SIZE  64
#define CCMD5_OUTPUT_SIZE 16
#define CCMD5_STATE_SIZE  16

const struct ccdigest_info *ccmd5_di(void);

#endif /* _CORECRYPTO_CCMD5_H_ */

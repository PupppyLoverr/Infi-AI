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
 * ccder.h: DER as the kernel's CoreEntitlements headers see it (EXTERNAL_HEADERS/CoreEntitlements/der_vm.h): a tag, and
 * a buffer being written or read as a pair of pointers. The DER functions themselves are not part of this interface.
 */
#ifndef _CORECRYPTO_CCDER_H_
#define _CORECRYPTO_CCDER_H_

#include <corecrypto/cc.h>

typedef unsigned long ccder_tag;

typedef struct ccder_blob {
	uint8_t *der;
	uint8_t *der_end;
} ccder_blob;

typedef struct ccder_read_blob {
	const uint8_t *der;
	const uint8_t *der_end;
} ccder_read_blob;

#endif /* _CORECRYPTO_CCDER_H_ */

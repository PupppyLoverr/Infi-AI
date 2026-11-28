// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* cczp.h: the header of a modulus (its size in units, bit length and arithmetic), followed by its units. */
#ifndef _CORECRYPTO_CCZP_H_
#define _CORECRYPTO_CCZP_H_

#include <corecrypto/ccn.h>

struct cczp_funcs;
typedef const struct cczp_funcs *cczp_funcs_t;

#define __CCZP_HEADER_ELEMENTS_DEFINITIONS(pre) \
	cc_size pre##n;                         \
	cc_unit pre##bitlen;                    \
	cczp_funcs_t pre##funcs;
#define __CCZP_ELEMENTS_DEFINITIONS(pre)    \
	__CCZP_HEADER_ELEMENTS_DEFINITIONS(pre) \
	cc_unit pre##ccn[];

struct cczp {
	__CCZP_ELEMENTS_DEFINITIONS()
} CC_ALIGNED(CCN_UNIT_SIZE);
typedef struct cczp *cczp_t;
typedef const struct cczp *cczp_const_t;

#endif /* _CORECRYPTO_CCZP_H_ */

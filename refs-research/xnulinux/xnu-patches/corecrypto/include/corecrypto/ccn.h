// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccn.h: multi-precision integers as arrays of units, and the sizes of such arrays. */
#ifndef _CORECRYPTO_CCN_H_
#define _CORECRYPTO_CCN_H_

#include <corecrypto/cc.h>

typedef size_t cc_size;
#if CCN_UNIT_SIZE == 8
typedef uint64_t cc_unit;
typedef int64_t  cc_int;
#define CCN_LOG2_BITS_PER_UNIT 6
#else
typedef uint32_t cc_unit;
typedef int32_t  cc_int;
#define CCN_LOG2_BITS_PER_UNIT 5
#endif

#define CCN_UNIT_BITS            (sizeof(cc_unit) * 8)
#define ccn_sizeof_n(_n_)        (sizeof(cc_unit) * (_n_))
#define ccn_nof(_bits_)          (((_bits_) + CCN_UNIT_BITS - 1) >> CCN_LOG2_BITS_PER_UNIT)
#define ccn_sizeof(_bits_)       (ccn_sizeof_n(ccn_nof(_bits_)))
#define ccn_nof_size(_size_)     (((_size_) + sizeof(cc_unit) - 1) / sizeof(cc_unit))
#define ccn_nof_sizeof(_expr_)   ccn_nof_size(sizeof(_expr_))
#define ccn_bitsof_n(_n_)        ((_n_) * CCN_UNIT_BITS)
#define ccn_bitsof_size(_size_)  ((_size_) * 8)
#define ccn_sizeof_size(_size_)  ccn_sizeof_n(ccn_nof_size(_size_))

#endif /* _CORECRYPTO_CCN_H_ */

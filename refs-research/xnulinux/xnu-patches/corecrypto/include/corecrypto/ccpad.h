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
 * ccpad.h: CBC with ciphertext stealing, CS3 of NIST SP 800-38A's addendum (Kerberos' variant: the last two blocks are
 * always swapped). They return the number of bytes processed.
 */
#ifndef _CORECRYPTO_CCPAD_H_
#define _CORECRYPTO_CCPAD_H_

#include <corecrypto/ccmode.h>

size_t ccpad_cts3_encrypt(const struct ccmode_cbc *cbc, cccbc_ctx *ctx, cccbc_iv *iv, size_t nbytes, const void *in,
    void *out);
size_t ccpad_cts3_decrypt(const struct ccmode_cbc *cbc, cccbc_ctx *ctx, cccbc_iv *iv, size_t nbytes, const void *in,
    void *out);

#endif /* _CORECRYPTO_CCPAD_H_ */

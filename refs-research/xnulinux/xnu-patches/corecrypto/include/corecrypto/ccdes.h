// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccdes.h: DES and two/three-key triple DES (FIPS 46-3), for the protocols that still name them. */
#ifndef _CORECRYPTO_CCDES_H_
#define _CORECRYPTO_CCDES_H_

#include <corecrypto/ccmode.h>

#define CCDES_BLOCK_SIZE 8
#define CCDES_KEY_SIZE   8

const struct ccmode_ecb *ccdes_ecb_encrypt_mode(void);
const struct ccmode_ecb *ccdes_ecb_decrypt_mode(void);
const struct ccmode_cbc *ccdes_cbc_encrypt_mode(void);
const struct ccmode_cbc *ccdes_cbc_decrypt_mode(void);
const struct ccmode_ecb *ccdes3_ecb_encrypt_mode(void);
const struct ccmode_ecb *ccdes3_ecb_decrypt_mode(void);
const struct ccmode_cbc *ccdes3_cbc_encrypt_mode(void);
const struct ccmode_cbc *ccdes3_cbc_decrypt_mode(void);

/* nonzero when a DES key is one of the weak or semi-weak keys */
int ccdes_key_is_weak(void *key, size_t length);
/* set each key byte's low bit so the byte has odd parity */
void ccdes_key_set_odd_parity(void *key, size_t length);

#endif /* _CORECRYPTO_CCDES_H_ */

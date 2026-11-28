// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccaes.h: AES (FIPS 197) in the modes of ccmode.h. */
#ifndef _CORECRYPTO_CCAES_H_
#define _CORECRYPTO_CCAES_H_

#include <corecrypto/cc_config.h>
#include <corecrypto/ccmode.h>

#define CCAES_BLOCK_SIZE   16
#define CCAES_KEY_SIZE_128 16
#define CCAES_KEY_SIZE_192 24
#define CCAES_KEY_SIZE_256 32

const struct ccmode_ecb *ccaes_ecb_encrypt_mode(void);
const struct ccmode_ecb *ccaes_ecb_decrypt_mode(void);
const struct ccmode_cbc *ccaes_cbc_encrypt_mode(void);
const struct ccmode_cbc *ccaes_cbc_decrypt_mode(void);
const struct ccmode_ctr *ccaes_ctr_crypt_mode(void);
const struct ccmode_xts *ccaes_xts_encrypt_mode(void);
const struct ccmode_xts *ccaes_xts_decrypt_mode(void);
const struct ccmode_gcm *ccaes_gcm_encrypt_mode(void);
const struct ccmode_gcm *ccaes_gcm_decrypt_mode(void);

#endif /* _CORECRYPTO_CCAES_H_ */

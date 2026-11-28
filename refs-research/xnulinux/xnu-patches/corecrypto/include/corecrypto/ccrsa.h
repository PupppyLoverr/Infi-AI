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
 * ccrsa.h: an RSA public key. Its context is the modulus' cczp header and units, then (in units) m0inv, the Montgomery
 * constant and the exponent; ccrsa_ctx_n() must be set (to the units the context has room for) before
 * ccrsa_make_pub() fills it.
 */
#ifndef _CORECRYPTO_CCRSA_H_
#define _CORECRYPTO_CCRSA_H_

#include <corecrypto/cc.h>
#include <corecrypto/ccn.h>
#include <corecrypto/cczp.h>

struct ccrsa_pub_ctx {
	__CCZP_ELEMENTS_DEFINITIONS(pb_)
} CC_ALIGNED(CCN_UNIT_SIZE);
typedef struct ccrsa_pub_ctx *ccrsa_pub_ctx_t;

#define ccrsa_pub_ctx_size(_nbytes_) (sizeof(struct cczp) + CCN_UNIT_SIZE + 3 * ccn_sizeof_size(_nbytes_))
#define ccrsa_pub_ctx_decl(_nbytes_, _name_)  cc_ctx_decl(struct ccrsa_pub_ctx, ccrsa_pub_ctx_size(_nbytes_), _name_)
#define ccrsa_pub_ctx_clear(_nbytes_, _name_) cc_clear(ccrsa_pub_ctx_size(_nbytes_), _name_)

#define ccrsa_ctx_zm(_ctx_) ((struct cczp *)(_ctx_))
#define ccrsa_ctx_n(_ctx_)  (ccrsa_ctx_zm(_ctx_)->n)

int ccrsa_make_pub(ccrsa_pub_ctx_t pubk, size_t exp_nbytes, const uint8_t *exp, size_t mod_nbytes, const uint8_t *mod);
/* PKCS #1 v1.5 (RFC 8017): *valid tells whether sig signs digest, of the hash with DER OID oid */
int ccrsa_verify_pkcs1v15(ccrsa_pub_ctx_t key, const uint8_t *oid, size_t digest_len, const uint8_t *digest,
    size_t sig_len, const uint8_t *sig, bool *valid);

#endif /* _CORECRYPTO_CCRSA_H_ */

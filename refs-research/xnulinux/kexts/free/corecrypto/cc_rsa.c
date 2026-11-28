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
 * RSA PKCS #1 v1.5 signature verification (Mbed TLS's). ccrsa_make_pub keeps the key's modulus and exponent, as bytes,
 * in the caller's context after its cczp header (whose n, set by the caller, bounds them); a verification imports them
 * into an Mbed TLS context.
 */
#include "cc_internal.h"
#include <mbedtls/rsa.h>
#include <mbedtls/md.h>

struct xc_rsa_key {
	uint32_t mod_nbytes;
	uint32_t exp_nbytes;
	uint8_t bytes[];	/* modulus, then exponent */
};

#define KEY(pubk) ((struct xc_rsa_key *)(void *)(pubk)->pb_ccn)

int
xc_rsa_make_pub(ccrsa_pub_ctx_t pubk, size_t exp_nbytes, const uint8_t *exp, size_t mod_nbytes, const uint8_t *mod)
{
	size_t room = 3 * ccn_sizeof_n(ccrsa_ctx_n(pubk));	/* the units after the header */

	while (mod_nbytes > 0 && mod[0] == 0) {
		mod++, mod_nbytes--;
	}
	while (exp_nbytes > 0 && exp[0] == 0) {
		exp++, exp_nbytes--;
	}
	if (mod_nbytes == 0 || exp_nbytes == 0 || mod_nbytes > ccn_sizeof_n(ccrsa_ctx_n(pubk)) ||
	    sizeof(struct xc_rsa_key) + mod_nbytes + exp_nbytes > room) {
		return CCRSA_INVALID_INPUT;
	}
	KEY(pubk)->mod_nbytes = (uint32_t)mod_nbytes;
	KEY(pubk)->exp_nbytes = (uint32_t)exp_nbytes;
	memcpy(KEY(pubk)->bytes, mod, mod_nbytes);
	memcpy(KEY(pubk)->bytes + mod_nbytes, exp, exp_nbytes);
	pubk->pb_bitlen = (cc_unit)mod_nbytes * 8;
	return 0;
}

static const struct {
	const unsigned char *oid;
	size_t oid_size;
	mbedtls_md_type_t md;
	size_t len;
} digests[] = {
	{ CC_DIGEST_OID_SHA1, 7, MBEDTLS_MD_SHA1, 20 },
	{ CC_DIGEST_OID_SHA224, 11, MBEDTLS_MD_SHA224, 28 },
	{ CC_DIGEST_OID_SHA256, 11, MBEDTLS_MD_SHA256, 32 },
	{ CC_DIGEST_OID_SHA384, 11, MBEDTLS_MD_SHA384, 48 },
	{ CC_DIGEST_OID_SHA512, 11, MBEDTLS_MD_SHA512, 64 },
	{ CC_DIGEST_OID_MD5, 10, MBEDTLS_MD_MD5, 16 },
};

/* oid: the digest's DER object identifier (tag and length first); NULL: the digest is signed as it is */
int
xc_rsa_verify_pkcs1v15(ccrsa_pub_ctx_t key, const uint8_t *oid, size_t digest_len, const uint8_t *digest,
    size_t sig_len, const uint8_t *sig, bool *valid)
{
	const struct xc_rsa_key *k = KEY(key);
	mbedtls_md_type_t md = MBEDTLS_MD_NONE;
	mbedtls_rsa_context rsa;
	int rc = 0;

	*valid = false;
	if (oid != NULL) {
		size_t i;
		/* a DER object identifier: tag 6 and a short-form length, then that many bytes */
		if (oid[0] != 0x06 || oid[1] > 0x7f) {
			return CCRSA_INVALID_INPUT;
		}
		for (i = 0; i < sizeof(digests) / sizeof(digests[0]); i++) {
			if ((size_t)oid[1] + 2 == digests[i].oid_size && memcmp(oid, digests[i].oid, digests[i].oid_size) == 0) {
				break;
			}
		}
		if (i == sizeof(digests) / sizeof(digests[0]) || digests[i].len != digest_len) {
			return CCRSA_INVALID_INPUT;
		}
		md = digests[i].md;
	}
	if (sig_len != k->mod_nbytes || digest_len > UINT32_MAX) {
		return 0;	/* a signature of the wrong size is just not valid */
	}
	mbedtls_rsa_init(&rsa);
	if (mbedtls_rsa_import_raw(&rsa, k->bytes, k->mod_nbytes, NULL, 0, NULL, 0, NULL, 0,
	    k->bytes + k->mod_nbytes, k->exp_nbytes) != 0 || mbedtls_rsa_complete(&rsa) != 0) {
		rc = CCRSA_KEY_ERROR;
	} else {
		*valid = mbedtls_rsa_rsassa_pkcs1_v15_verify(&rsa, md, (unsigned int)digest_len, digest, sig) == 0;
	}
	mbedtls_rsa_free(&rsa);
	return rc;
}

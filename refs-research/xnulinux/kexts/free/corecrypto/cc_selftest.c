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
 * Known-answer tests run when the kext starts, through the same calls the kernel makes: the digests on "abc" (FIPS
 * 180-4 / RFC 1321), HMAC-SHA-256 (RFC 4231 case 1), AES-128 (FIPS 197 C.1) in ECB, CBC and GCM (the GCM
 * specification's test case 2), and round trips of the modes without a short standard vector here. The full test
 * vectors run in user space (test/).
 */
#include "cc_internal.h"

static int
hexeq(const uint8_t *got, const char *hex, size_t n)
{
	for (size_t i = 0; i < n; i++) {
		unsigned v;
		char c[3] = { hex[2 * i], hex[2 * i + 1], 0 };
		v = (unsigned)((c[0] <= '9' ? c[0] - '0' : (c[0] | 0x20) - 'a' + 10) << 4 |
		    (c[1] <= '9' ? c[1] - '0' : (c[1] | 0x20) - 'a' + 10));
		if (got[i] != v) {
			return 0;
		}
	}
	return 1;
}

static int
digest_abc(const struct ccdigest_info *di, const char *hex)
{
	uint8_t out[64];

	xc_digest(di, 3, "abc", out);
	return hexeq(out, hex, di->output_size);
}

int
xc_selftest(void)
{
	static const uint8_t k128[16] = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15 };
	static const uint8_t pt[16] = {
		0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
	};
	uint8_t out[64], back[64], key[20], tag[16], iv[16];

	if (!digest_abc(xc_md5_di(), "900150983cd24fb0d6963f7d28e17f72")) {
		return 1;
	}
	if (!digest_abc(xc_sha1_di(), "a9993e364706816aba3e25717850c26c9cd0d89d")) {
		return 2;
	}
	if (!digest_abc(xc_sha224_di(), "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7")) {
		return 3;
	}
	if (!digest_abc(xc_sha256_di(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")) {
		return 4;
	}
	if (!digest_abc(xc_sha384_di(), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded163"
	    "1a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7")) {
		return 5;
	}
	if (!digest_abc(xc_sha512_di(), "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a"
	    "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f")) {
		return 6;
	}
	memset(key, 0x0b, sizeof(key));
	xc_hmac(xc_sha256_di(), sizeof(key), key, 8, "Hi There", out);
	if (!hexeq(out, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7", 32)) {
		return 7;
	}

	/* AES-128 */
	{
		const struct ccmode_ecb *e = xc_aes_ecb_encrypt(), *d = xc_aes_ecb_decrypt();
		ccecb_ctx_decl(e->size, ce);
		ccecb_ctx_decl(d->size, cd);
		if (e->init(e, ce, 16, k128) || e->ecb(ce, 1, pt, out) || !hexeq(out, "69c4e0d86a7b0430d8cdb78070b4c55a", 16)) {
			return 8;
		}
		if (d->init(d, cd, 16, k128) || d->ecb(cd, 1, out, back) || memcmp(back, pt, 16) != 0) {
			return 9;
		}
	}
	{
		/* CBC with a zero IV: the first block is ECB's */
		const struct ccmode_cbc *e = xc_aes_cbc_encrypt(), *d = xc_aes_cbc_decrypt();
		cccbc_ctx_decl(e->size, ce);
		cccbc_ctx_decl(d->size, cd);
		uint8_t two[32];
		memcpy(two, pt, 16);
		memcpy(two + 16, pt, 16);
		memset(iv, 0, 16);
		if (e->init(e, ce, 16, k128) || e->cbc(ce, (cccbc_iv *)iv, 2, two, out) ||
		    !hexeq(out, "69c4e0d86a7b0430d8cdb78070b4c55a", 16) || memcmp(iv, out + 16, 16) != 0) {
			return 10;
		}
		memset(iv, 0, 16);
		if (d->init(d, cd, 16, k128) || d->cbc(cd, (cccbc_iv *)iv, 2, out, back) || memcmp(back, two, 32) != 0) {
			return 11;
		}
	}
	{
		/* GCM test case 2: zero key, zero 96-bit IV, one zero block */
		const struct ccmode_gcm *e = xc_aes_gcm_encrypt(), *d = xc_aes_gcm_decrypt();
		ccgcm_ctx_decl(e->size, ge);
		ccgcm_ctx_decl(d->size, gd);
		uint8_t zero[16] = {0};
		if (e->init(e, ge, 16, zero) || e->set_iv(ge, 12, zero) || e->gcm(ge, 16, zero, out) ||
		    e->finalize(ge, 16, tag) || !hexeq(out, "0388dace60b6a392f328c2b971b2fe78", 16) ||
		    !hexeq(tag, "ab6e47d42cec13bdf53a67b21257bddf", 16)) {
			return 12;
		}
		if (d->init(d, gd, 16, zero) || d->set_iv(gd, 12, zero) || d->gcm(gd, 16, out, back) ||
		    d->finalize(gd, 16, tag) || memcmp(back, zero, 16) != 0) {
			return 13;
		}
		tag[0] ^= 1;
		if (d->reset(gd) || d->set_iv(gd, 12, zero) || d->gcm(gd, 16, out, back) ||
		    d->finalize(gd, 16, tag) != CCMODE_INTEGRITY_FAILURE) {
			return 14;
		}
	}
	{
		/* XTS and CTR: back to the plaintext */
		const struct ccmode_xts *e = xc_aes_xts_encrypt(), *d = xc_aes_xts_decrypt();
		ccxts_ctx_decl(e->size, xe);
		ccxts_ctx_decl(d->size, xd);
		ccxts_tweak_decl(e->tweak_size, t);
		uint8_t k2[16], data[64];
		for (int i = 0; i < 16; i++) {
			k2[i] = (uint8_t)(0xf0 + i);
		}
		for (int i = 0; i < 64; i++) {
			data[i] = (uint8_t)i;
		}
		memset(iv, 0, 16);
		if (e->init(e, xe, 16, k128, k2) || e->set_tweak(xe, t, iv) || !e->xts(xe, t, 4, data, out) ||
		    memcmp(out, data, 64) == 0) {
			return 15;
		}
		if (d->init(d, xd, 16, k128, k2) || d->set_tweak(xd, t, iv) || !d->xts(xd, t, 4, out, back) ||
		    memcmp(back, data, 64) != 0) {
			return 16;
		}
		const struct ccmode_ctr *c = xc_aes_ctr();
		ccctr_ctx_decl(c->size, cc);
		if (c->init(c, cc, 16, k128, iv) || c->ctr(cc, 7, data, out) || c->ctr(cc, 57, data + 7, out + 7) ||
		    c->setctr(c, cc, iv) || c->ctr(cc, 64, out, back) || memcmp(back, data, 64) != 0) {
			return 17;
		}
	}
	{
		/* ChaCha20-Poly1305 and triple DES with CTS3: round trips, and a forged tag refused */
		ccchacha20poly1305_ctx cp;
		uint8_t k32[32], nonce[12] = {0}, msg[40];
		for (int i = 0; i < 32; i++) {
			k32[i] = (uint8_t)i;
		}
		for (int i = 0; i < 40; i++) {
			msg[i] = (uint8_t)(i * 7);
		}
		if (xc_ccp_init(NULL, &cp, k32) || xc_ccp_setnonce(NULL, &cp, nonce) || xc_ccp_aad(NULL, &cp, 3, "abc") ||
		    xc_ccp_encrypt(NULL, &cp, 40, msg, out) || xc_ccp_finalize(NULL, &cp, tag)) {
			return 18;
		}
		if (xc_ccp_reset(NULL, &cp) || xc_ccp_setnonce(NULL, &cp, nonce) || xc_ccp_aad(NULL, &cp, 3, "abc") ||
		    xc_ccp_decrypt(NULL, &cp, 40, out, back) || xc_ccp_verify(NULL, &cp, tag) || memcmp(back, msg, 40) != 0) {
			return 19;
		}
		tag[15] ^= 0x80;
		if (xc_ccp_reset(NULL, &cp) || xc_ccp_setnonce(NULL, &cp, nonce) || xc_ccp_aad(NULL, &cp, 3, "abc") ||
		    xc_ccp_decrypt(NULL, &cp, 40, out, back) || xc_ccp_verify(NULL, &cp, tag) != CCMODE_INTEGRITY_FAILURE) {
			return 20;
		}
		const struct ccmode_cbc *e = xc_des3_cbc_encrypt(), *d = xc_des3_cbc_decrypt();
		cccbc_ctx_decl(e->size, de);
		cccbc_ctx_decl(d->size, dd);
		uint8_t k24[24];
		for (int i = 0; i < 24; i++) {
			k24[i] = (uint8_t)(0x13 * i + 1);
		}
		xc_des_key_set_odd_parity(k24, 24);
		memset(iv, 0, 8);
		if (e->init(e, de, 24, k24) || xc_cts3_encrypt(e, de, (cccbc_iv *)iv, 21, msg, out) != 21) {
			return 21;
		}
		memset(iv, 0, 8);
		if (d->init(d, dd, 24, k24) || xc_cts3_decrypt(d, dd, (cccbc_iv *)iv, 21, out, back) != 21 ||
		    memcmp(back, msg, 21) != 0) {
			return 22;
		}
	}
	return 0;
}

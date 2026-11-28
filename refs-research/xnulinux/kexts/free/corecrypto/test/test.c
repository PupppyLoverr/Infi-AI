// SPDX-License-Identifier: APSL-2.0
/*
 * The corecrypto kext's primitives in user space, through the interface's calls: its own self tests, RFC 3962's CTS
 * vectors, and every mode against OpenSSL on random keys, lengths and call splits. run.sh builds and runs it.
 */
#include "../cc_internal.h"
#include <openssl/evp.h>
#include <openssl/hmac.h>
#include <openssl/rsa.h>
#include <openssl/bn.h>
#include <openssl/core_names.h>
#include <openssl/provider.h>
#include <sys/random.h>
#include "rfc3962.h"

/* the kernel's (osfmk/xnulinux_crypto) */
void
cc_clear(size_t len, void *dst)
{
	memset(dst, 0, len);
	__asm__ __volatile__ ("" : : "r"(dst) : "memory");
}

int
cc_cmp_safe(size_t num, const void *a, const void *b)
{
	const volatile uint8_t *x = a, *y = b;
	uint8_t d = 0;
	for (size_t i = 0; i < num; i++) {
		d |= x[i] ^ y[i];
	}
	return num == 0 || d != 0;
}

static unsigned long host_calls;	/* how often the kext asked the host for random numbers */
void
xc_random(void *buf, size_t n)
{
	host_calls++;
	for (size_t k; n; buf = (uint8_t *)buf + k, n -= k) {	/* getentropy: Linux's and macOS's alike, 256 bytes a call */
		k = n < 256 ? n : 256;
		if (getentropy(buf, k) != 0) {
			abort();
		}
	}
}

/* the allocations live now and their sizes: a free of another size fails the test, as IOFreeData's size must match */
static struct { void *p; size_t n; } live[4096];
static int frees_wrong;
void
xc_test_alloc(void *p, size_t n)
{
	for (size_t i = 0; i < sizeof(live) / sizeof(live[0]); i++) {
		if (live[i].p == NULL) {
			live[i].p = p, live[i].n = n;
			return;
		}
	}
	abort();
}
void
xc_test_free(void *p, size_t n)
{
	for (size_t i = 0; i < sizeof(live) / sizeof(live[0]); i++) {
		if (live[i].p == p) {
			frees_wrong += live[i].n != n;
			live[i].p = NULL;
			free(p);
			return;
		}
	}
	abort();
}

/* the context macros take expressions as sizes */
_Static_assert(cc_ctx_n(ccecb_ctx, 1 ? 17 : 32) == 2, "cc_ctx_n with a conditional size");
_Static_assert(cc_pad_align(1 ? 1 : 2) == CC_MAX_ALIGNMENT, "cc_pad_align with a conditional size");

static uint64_t seed = 0x9e3779b97f4a7c15ull;
static uint64_t
rnd(void)
{
	seed ^= seed << 13, seed ^= seed >> 7, seed ^= seed << 17;
	return seed;
}
static size_t rndn(size_t n) { return n ? (size_t)(rnd() % n) : 0; }
static void fill(uint8_t *p, size_t n) { while (n--) *p++ = (uint8_t)rnd(); }

static int failures;
#define CHECK(cond, ...) do { if (!(cond)) { failures++; printf("FAIL %s:%d: ", __FILE__, __LINE__); printf(__VA_ARGS__); printf("\n"); } } while (0)

static void
unhex(const char *h, uint8_t *out, size_t *n)
{
	size_t i = 0;
	for (; h[2 * i]; i++) {
		sscanf(h + 2 * i, "%2hhx", &out[i]);
	}
	*n = i;
}

/* digests and HMAC, in random pieces */
static void
test_digests(void)
{
	static const struct { const struct ccdigest_info *(*di)(void); const char *name; } d[] = {
		{ xc_md5_di, "MD5" }, { xc_sha1_di, "SHA1" }, { xc_sha224_di, "SHA224" }, { xc_sha256_di, "SHA256" },
		{ xc_sha384_di, "SHA384" }, { xc_sha512_di, "SHA512" },
	};
	static uint8_t data[5000], key[300];

	for (size_t a = 0; a < sizeof(d) / sizeof(d[0]); a++) {
		const struct ccdigest_info *di = d[a].di();
		const EVP_MD *md = EVP_get_digestbyname(d[a].name);
		for (int it = 0; it < 400; it++) {
			size_t len = it < 300 ? (size_t)it : rndn(sizeof(data)), klen = rndn(sizeof(key)), off = 0;
			uint8_t ours[64], theirs[64];
			unsigned tl;
			fill(data, len);
			fill(key, klen);
			ccdigest_di_decl(di, ctx);
			memset(ctx, 0xa5, ccdigest_di_size(di));
			xc_digest_init(di, ctx);
			/* MD5Init and SHA1Init copy the whole pending block out of a fresh context */
			CHECK(memcmp(ccdigest_data(di, ctx), (uint8_t[128]){0}, di->block_size) == 0, "%s init leaves the block", d[a].name);
			while (off < len) {
				size_t n = 1 + rndn(len - off);
				xc_digest_update(di, ctx, n, data + off);
				off += n;
			}
			xc_digest_final(di, ctx, ours);
			EVP_Digest(data, len, theirs, &tl, md, NULL);
			CHECK(tl == di->output_size && memcmp(ours, theirs, tl) == 0, "%s of %zu bytes", d[a].name, len);
			cchmac_di_decl(di, hc);
			xc_hmac_init(di, hc, klen, key);
			for (off = 0; off < len;) {
				size_t n = 1 + rndn(len - off);
				xc_hmac_update(di, hc, n, data + off);
				off += n;
			}
			xc_hmac_final(di, hc, ours);
			HMAC(md, key, (int)klen, data, len, theirs, &tl);
			CHECK(memcmp(ours, theirs, tl) == 0, "HMAC-%s key %zu data %zu", d[a].name, klen, len);
		}
	}
}

static int
evp_crypt(const EVP_CIPHER *c, int enc, const uint8_t *key, const uint8_t *iv, const uint8_t *in, size_t n, uint8_t *out)
{
	EVP_CIPHER_CTX *x = EVP_CIPHER_CTX_new();
	int l1 = 0, l2 = 0;
	EVP_CipherInit_ex(x, c, NULL, key, iv, enc);
	EVP_CIPHER_CTX_set_padding(x, 0);
	EVP_CipherUpdate(x, out, &l1, in, (int)n);
	EVP_CipherFinal_ex(x, out + l1, &l2);
	EVP_CIPHER_CTX_free(x);
	return l1 + l2;
}

static void
test_aes(void)
{
	static uint8_t key[32], key2[32], iv[16], ivc[16], pt[4096], ours[4096], theirs[4096], back[4096];
	const char *bits[] = { "128", "192", "256" };

	for (int k = 0; k < 3; k++) {
		size_t kn = 16 + 8 * (size_t)k;
		char name[32];
		for (int it = 0; it < 200; it++) {
			size_t nb = 1 + rndn(64), n = nb * 16;
			fill(key, kn), fill(key2, kn), fill(iv, 16), fill(pt, n);
			/* ECB */
			const struct ccmode_ecb *ee = xc_aes_ecb_encrypt(), *ed = xc_aes_ecb_decrypt();
			ccecb_ctx_decl(ee->size, ce);
			ccecb_ctx_decl(ed->size, cd);
			ee->init(ee, ce, kn, key), ee->ecb(ce, nb, pt, ours);
			snprintf(name, sizeof(name), "AES-%s-ECB", bits[k]);
			evp_crypt(EVP_get_cipherbyname(name), 1, key, NULL, pt, n, theirs);
			CHECK(memcmp(ours, theirs, n) == 0, "%s %zu", name, n);
			ed->init(ed, cd, kn, key), ed->ecb(cd, nb, ours, back);
			CHECK(memcmp(back, pt, n) == 0, "%s decrypt", name);
			/* CBC, in two calls, the IV carried */
			const struct ccmode_cbc *be = xc_aes_cbc_encrypt(), *bd = xc_aes_cbc_decrypt();
			cccbc_ctx_decl(be->size, cbe);
			cccbc_ctx_decl(bd->size, cbd);
			size_t split = rndn(nb + 1);
			memcpy(ivc, iv, 16);
			be->init(be, cbe, kn, key);
			be->cbc(cbe, (cccbc_iv *)ivc, split, pt, ours), be->cbc(cbe, (cccbc_iv *)ivc, nb - split, pt + split * 16, ours + split * 16);
			snprintf(name, sizeof(name), "AES-%s-CBC", bits[k]);
			evp_crypt(EVP_get_cipherbyname(name), 1, key, iv, pt, n, theirs);
			CHECK(memcmp(ours, theirs, n) == 0 && memcmp(ivc, theirs + n - 16, 16) == 0, "%s %zu", name, n);
			memcpy(ivc, iv, 16);
			memcpy(back, ours, n);
			bd->init(bd, cbd, kn, key), bd->cbc(cbd, (cccbc_iv *)ivc, nb, back, back);	/* in place */
			CHECK(memcmp(back, pt, n) == 0, "%s decrypt in place", name);
			/* CTR in byte pieces */
			const struct ccmode_ctr *ct = xc_aes_ctr();
			ccctr_ctx_decl(ct->size, cc);
			size_t len = rndn(n + 1), off = 0;
			ct->init(ct, cc, kn, key, iv);
			while (off < len) {
				size_t m = 1 + rndn(len - off);
				ct->ctr(cc, m, pt + off, ours + off);
				off += m;
			}
			snprintf(name, sizeof(name), "AES-%s-CTR", bits[k]);
			evp_crypt(EVP_get_cipherbyname(name), 1, key, iv, pt, len, theirs);
			CHECK(memcmp(ours, theirs, len) == 0, "%s %zu", name, len);
			/* XTS (128 and 256), the sector in two calls */
			if (kn != 24) {
				const struct ccmode_xts *xe = xc_aes_xts_encrypt(), *xd = xc_aes_xts_decrypt();
				ccxts_ctx_decl(xe->size, cxe);
				ccxts_ctx_decl(xd->size, cxd);
				ccxts_tweak_decl(xe->tweak_size, tw);
				uint8_t k2[64];
				memcpy(k2, key, kn), memcpy(k2 + kn, key2, kn);
				xe->init(xe, cxe, kn, key, key2), xe->set_tweak(cxe, tw, iv);
				xe->xts(cxe, tw, split, pt, ours), xe->xts(cxe, tw, nb - split, pt + split * 16, ours + split * 16);
				snprintf(name, sizeof(name), "AES-%s-XTS", kn == 16 ? "128" : "256");
				evp_crypt(EVP_get_cipherbyname(name), 1, k2, iv, pt, n, theirs);
				CHECK(memcmp(ours, theirs, n) == 0, "%s %zu", name, n);
				xd->init(xd, cxd, kn, key, key2), xd->set_tweak(cxd, tw, iv), xd->xts(cxd, tw, nb, ours, back);
				CHECK(memcmp(back, pt, n) == 0, "%s decrypt", name);
			}
		}
	}
	/* XTS takes 16- and 32-byte keys, and two different ones */
	{
		const struct ccmode_xts *xe = xc_aes_xts_encrypt();
		ccxts_ctx_decl(xe->size, cx);
		uint8_t k1[32], k2[32];
		fill(k1, 32), fill(k2, 32);
		CHECK(xe->init(xe, cx, 24, k1, k2) != 0, "XTS with 24-byte keys accepted");
		CHECK(xe->init(xe, cx, (size_t)-16, k1, k2) != 0, "XTS with a negative key length accepted");
		CHECK(xe->init(xe, cx, 0, k1, k2) != 0, "XTS with no key accepted");
		CHECK(xe->init(xe, cx, 32, k1, k1) == CCERR_XTS_KEYS_EQUAL, "XTS with equal keys accepted");
		CHECK(xe->init(xe, cx, 32, k1, k2) == 0, "XTS-256 init");
	}
	/* a block count whose bytes size_t cannot hold: refused, nothing done */
	{
		const struct ccmode_cbc *ce = xc_aes_cbc_encrypt();
		cccbc_ctx_decl(ce->size, cc);
		uint8_t k1[16] = {0}, ivb[16] = {0}, buf[16] = {0};
		ce->init(ce, cc, 16, k1);
		CHECK(ce->cbc(cc, (cccbc_iv *)ivb, SIZE_MAX / 16 + 1, buf, buf) != 0, "AES-CBC with a block count past size_t");
		const struct ccmode_cbc *de = xc_des_cbc_encrypt(), *d3 = xc_des3_cbc_encrypt();
		cccbc_ctx_decl(de->size, dc);
		cccbc_ctx_decl(d3->size, d3c);
		uint8_t dk[24] = { 0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01,
			0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23 };
		de->init(de, dc, 8, dk), d3->init(d3, d3c, 24, dk);
		CHECK(de->cbc(dc, (cccbc_iv *)ivb, SIZE_MAX / 8 + 1, buf, buf) != 0, "DES-CBC with a block count past size_t");
		CHECK(d3->cbc(d3c, (cccbc_iv *)ivb, SIZE_MAX / 8 + 1, buf, buf) != 0, "3DES-CBC with a block count past size_t");
		CHECK(memcmp(buf, (uint8_t[16]){0}, 16) == 0, "CBC wrote with a block count past size_t");
	}
}

static void
test_gcm(void)
{
	static uint8_t key[32], iv[64], aad[300], pt[3000], ours[3000], theirs[3000], back[3000];

	for (int it = 0; it < 600; it++) {
		size_t kn = 16 + 8 * rndn(3), ivn = it % 3 ? 12 : 1 + rndn(64), an = rndn(sizeof(aad)), n = rndn(sizeof(pt));
		size_t tn = 12 + rndn(5), off;
		uint8_t tag[16], ttag[16];
		int l;
		fill(key, kn), fill(iv, ivn), fill(aad, an), fill(pt, n);
		const struct ccmode_gcm *e = xc_aes_gcm_encrypt(), *d = xc_aes_gcm_decrypt();
		ccgcm_ctx_decl(e->size, ge);
		ccgcm_ctx_decl(d->size, gd);
		CHECK(e->init(e, ge, kn, key) == 0 && e->set_iv(ge, ivn, iv) == 0, "GCM init");
		for (off = 0; off < an;) {
			size_t m = 1 + rndn(an - off);
			e->gmac(ge, m, aad + off);
			off += m;
		}
		for (off = 0; off < n;) {
			size_t m = 1 + rndn(n - off);
			e->gcm(ge, m, pt + off, ours + off);
			off += m;
		}
		CHECK(e->finalize(ge, tn, tag) == 0, "GCM finalize");
		EVP_CIPHER_CTX *x = EVP_CIPHER_CTX_new();
		EVP_EncryptInit_ex(x, kn == 16 ? EVP_aes_128_gcm() : kn == 24 ? EVP_aes_192_gcm() : EVP_aes_256_gcm(), NULL, NULL, NULL);
		EVP_CIPHER_CTX_ctrl(x, EVP_CTRL_GCM_SET_IVLEN, (int)ivn, NULL);
		EVP_EncryptInit_ex(x, NULL, NULL, key, iv);
		if (an) EVP_EncryptUpdate(x, NULL, &l, aad, (int)an);
		if (n) EVP_EncryptUpdate(x, theirs, &l, pt, (int)n);
		EVP_EncryptFinal_ex(x, theirs, &l);
		EVP_CIPHER_CTX_ctrl(x, EVP_CTRL_GCM_GET_TAG, 16, ttag);
		EVP_CIPHER_CTX_free(x);
		CHECK(memcmp(ours, theirs, n) == 0 && memcmp(tag, ttag, tn) == 0, "GCM k%zu iv%zu aad%zu n%zu", kn, ivn, an, n);
		d->init(d, gd, kn, key), d->set_iv(gd, ivn, iv), d->gmac(gd, an, aad), d->gcm(gd, n, ours, back);
		CHECK(d->finalize(gd, tn, tag) == 0 && memcmp(back, pt, n) == 0, "GCM decrypt");
		tag[rndn(tn)] ^= (uint8_t)(1 << rndn(8));
		d->reset(gd), d->set_iv(gd, ivn, iv), d->gmac(gd, an, aad), d->gcm(gd, n, ours, back);
		CHECK(d->finalize(gd, tn, tag) == CCMODE_INTEGRITY_FAILURE, "GCM forged tag accepted");
		/* tag lengths: SP 800-38D's only, both ways */
		size_t bad = (size_t[]){ 0, 1, 3, 5, 7, 9, 11, 17 }[rndn(8)], good = it & 1 ? 4 : 8;
		e->reset(ge), e->set_iv(ge, ivn, iv), e->gmac(ge, an, aad), e->gcm(ge, n, pt, ours);
		CHECK(e->finalize(ge, bad, tag) == CCMODE_INVALID_INPUT, "GCM encrypt with a %zu-byte tag", bad);
		d->reset(gd), d->set_iv(gd, ivn, iv), d->gmac(gd, an, aad), d->gcm(gd, n, theirs, back);
		CHECK(d->finalize(gd, bad, ttag) == CCMODE_INVALID_INPUT, "GCM decrypt with a %zu-byte tag", bad);
		d->reset(gd), d->set_iv(gd, ivn, iv), d->gmac(gd, an, aad), d->gcm(gd, n, theirs, back);
		CHECK(d->finalize(gd, good, ttag) == 0, "GCM decrypt with a %zu-byte tag", good);
	}
	/* IPsec's sequence: init with an IV, then for each packet reset, inc_iv, aad, text, finalize */
	{
		const struct ccmode_gcm *e = xc_aes_gcm_encrypt();
		ccgcm_ctx_decl(e->size, ge);
		uint8_t iv0[12], cur[12], tag[16], ttag[16];
		int l;
		fill(key, 32), fill(iv0, 12), fill(pt, 100);
		memcpy(cur, iv0, 12);
		CHECK(xc_gcm_init_with_iv(e, ge, 32, key, iv0) == 0, "init_with_iv");
		CHECK(xc_gcm_inc_iv(e, ge, iv) == CCMODE_INVALID_CALL_SEQUENCE, "inc_iv before reset");
		for (int p = 0; p < 300; p++) {
			e->reset(ge);
			CHECK(xc_gcm_inc_iv(e, ge, iv) == 0, "inc_iv");
			for (int i = 11; i >= 4; i--) if (++cur[i]) break;
			CHECK(memcmp(iv, cur, 12) == 0, "inc_iv counts the last 8 bytes up (packet %d)", p);
			e->gmac(ge, 8, aad), e->gcm(ge, 100, pt, ours), e->finalize(ge, 16, tag);
			EVP_CIPHER_CTX *x = EVP_CIPHER_CTX_new();
			EVP_EncryptInit_ex(x, EVP_aes_256_gcm(), NULL, key, cur);
			EVP_EncryptUpdate(x, NULL, &l, aad, 8), EVP_EncryptUpdate(x, theirs, &l, pt, 100), EVP_EncryptFinal_ex(x, theirs, &l);
			EVP_CIPHER_CTX_ctrl(x, EVP_CTRL_GCM_GET_TAG, 16, ttag);
			EVP_CIPHER_CTX_free(x);
			CHECK(memcmp(ours, theirs, 100) == 0 && memcmp(tag, ttag, 16) == 0, "IPsec packet %d", p);
		}
		/* the count stays in the last 8 bytes: it wraps there */
		memset(iv0, 0x5a, 4), memset(iv0 + 4, 0xff, 8);
		xc_gcm_init_with_iv(e, ge, 32, key, iv0), e->reset(ge);
		CHECK(xc_gcm_inc_iv(e, ge, iv) == 0 && memcmp(iv, "\x5a\x5a\x5a\x5a\0\0\0\0\0\0\0\0", 12) == 0, "inc_iv wraps");
	}
}

static void
test_des(void)
{
	static uint8_t key[24], iv[8], ivc[8], pt[800], ours[800], theirs[800], back[800];

	for (int it = 0; it < 300; it++) {
		size_t nb = 1 + rndn(100), n = nb * 8;
		fill(key, 24), fill(iv, 8), fill(pt, n);
		xc_des_key_set_odd_parity(key, 24);
		struct { const struct ccmode_cbc *e, *d; size_t kn; const char *name; } m[] = {
			{ xc_des_cbc_encrypt(), xc_des_cbc_decrypt(), 8, "DES-CBC" },
			{ xc_des3_cbc_encrypt(), xc_des3_cbc_decrypt(), 24, "DES-EDE3-CBC" },
			{ xc_des3_cbc_encrypt(), xc_des3_cbc_decrypt(), 16, "DES-EDE-CBC" },
		};
		for (int j = 0; j < 3; j++) {
			cccbc_ctx_decl(m[j].e->size, ce);
			cccbc_ctx_decl(m[j].d->size, cd);
			memcpy(ivc, iv, 8);
			m[j].e->init(m[j].e, ce, m[j].kn, key), m[j].e->cbc(ce, (cccbc_iv *)ivc, nb, pt, ours);
			const EVP_CIPHER *c = EVP_CIPHER_fetch(NULL, m[j].name, NULL);
			CHECK(c != NULL, "OpenSSL has no %s", m[j].name);
			if (c) {
				evp_crypt(c, 1, key, iv, pt, n, theirs);
				CHECK(memcmp(ours, theirs, n) == 0, "%s %zu", m[j].name, n);
				EVP_CIPHER_free((EVP_CIPHER *)c);
			}
			memcpy(ivc, iv, 8);
			m[j].d->init(m[j].d, cd, m[j].kn, key), m[j].d->cbc(cd, (cccbc_iv *)ivc, nb, ours, back);
			CHECK(memcmp(back, pt, n) == 0, "%s decrypt", m[j].name);
		}
		const struct ccmode_ecb *ee = xc_des3_ecb_encrypt();
		ccecb_ctx_decl(ee->size, ce);
		ee->init(ee, ce, 24, key), ee->ecb(ce, nb, pt, ours);
		evp_crypt(EVP_des_ede3_ecb(), 1, key, NULL, pt, n, theirs);
		CHECK(memcmp(ours, theirs, n) == 0, "DES-EDE3-ECB");
	}
	static const uint8_t weak[8] = { 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01 };
	uint8_t k[8] = { 0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1 };
	CHECK(xc_des_key_is_weak((void *)weak, 8) && !xc_des_key_is_weak(k, 8), "DES weak keys");
	/* the same weak and semi-weak keys with their parity bits flipped: the bits are not part of the key */
	static const uint8_t weak_even[8] = { 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00 };
	static const uint8_t semi[8] = { 0x01, 0x1f, 0x01, 0x1f, 0x01, 0x0e, 0x01, 0x0e };
	uint8_t semi_even[8], k3[24];
	for (int i = 0; i < 8; i++) semi_even[i] = semi[i] ^ 1;
	CHECK(xc_des_key_is_weak((void *)weak_even, 8), "DES weak key without odd parity");
	CHECK(xc_des_key_is_weak((void *)semi, 8) && xc_des_key_is_weak(semi_even, 8), "DES semi-weak key, both parities");
	memcpy(k3, k, 8), memcpy(k3 + 8, k, 8), memcpy(k3 + 16, weak_even, 8);
	CHECK(xc_des_key_is_weak(k3, 24), "triple DES key with a weak third part");
	CHECK(memcmp(weak_even, (uint8_t[8]){0}, 8) == 0, "the key checked is left as it was");
}

static void
test_cts3(void)
{
	uint8_t key[16], in[64], out[64], ours[64], back[64], next[16], iv[16];
	size_t n, kn;
	unhex("636869636b656e207465726979616b69", key, &kn);
	const struct ccmode_cbc *e = xc_aes_cbc_encrypt(), *d = xc_aes_cbc_decrypt();
	cccbc_ctx_decl(e->size, ce);
	cccbc_ctx_decl(d->size, cd);
	e->init(e, ce, 16, key), d->init(d, cd, 16, key);
	for (size_t i = 0; i < sizeof(rfc3962) / sizeof(rfc3962[0]); i++) {
		size_t on, nn;
		unhex(rfc3962[i].in, in, &n), unhex(rfc3962[i].out, out, &on), unhex(rfc3962[i].next_iv, next, &nn);
		memset(iv, 0, 16);
		CHECK(xc_cts3_encrypt(e, ce, (cccbc_iv *)iv, n, in, ours) == n && memcmp(ours, out, n) == 0, "RFC 3962 case %zu", i);
		CHECK(memcmp(iv, next, 16) == 0, "RFC 3962 case %zu next IV", i);
		memset(iv, 0, 16);
		CHECK(xc_cts3_decrypt(d, cd, (cccbc_iv *)iv, n, out, back) == n && memcmp(back, in, n) == 0, "RFC 3962 case %zu decrypt", i);
		CHECK(memcmp(iv, next, 16) == 0, "RFC 3962 case %zu decrypt next IV", i);
	}
}

static void
test_chachapoly(void)
{
	static uint8_t key[32], nonce[12], aad[200], pt[2000], ours[2000], theirs[2000], back[2000];
	for (int it = 0; it < 400; it++) {
		size_t an = rndn(sizeof(aad)), n = rndn(sizeof(pt)), off;
		uint8_t tag[16], ttag[16];
		int l;
		ccchacha20poly1305_ctx c;
		fill(key, 32), fill(nonce, 12), fill(aad, an), fill(pt, n);
		xc_ccp_init(NULL, &c, key), xc_ccp_setnonce(NULL, &c, nonce);
		for (off = 0; off < an;) { size_t m = 1 + rndn(an - off); xc_ccp_aad(NULL, &c, m, aad + off); off += m; }
		for (off = 0; off < n;) { size_t m = 1 + rndn(n - off); xc_ccp_encrypt(NULL, &c, m, pt + off, ours + off); off += m; }
		CHECK(xc_ccp_finalize(NULL, &c, tag) == 0, "ChaCha20-Poly1305 finalize");
		EVP_CIPHER_CTX *x = EVP_CIPHER_CTX_new();
		EVP_EncryptInit_ex(x, EVP_chacha20_poly1305(), NULL, key, nonce);
		if (an) EVP_EncryptUpdate(x, NULL, &l, aad, (int)an);
		if (n) EVP_EncryptUpdate(x, theirs, &l, pt, (int)n);
		EVP_EncryptFinal_ex(x, theirs, &l);
		EVP_CIPHER_CTX_ctrl(x, EVP_CTRL_AEAD_GET_TAG, 16, ttag);
		EVP_CIPHER_CTX_free(x);
		CHECK(memcmp(ours, theirs, n) == 0 && memcmp(tag, ttag, 16) == 0, "ChaCha20-Poly1305 aad %zu n %zu", an, n);
		xc_ccp_reset(NULL, &c), xc_ccp_setnonce(NULL, &c, nonce), xc_ccp_aad(NULL, &c, an, aad);
		xc_ccp_decrypt(NULL, &c, n, ours, back);
		CHECK(xc_ccp_verify(NULL, &c, tag) == 0 && memcmp(back, pt, n) == 0, "ChaCha20-Poly1305 decrypt");
		tag[rndn(16)] ^= 4;
		xc_ccp_reset(NULL, &c), xc_ccp_setnonce(NULL, &c, nonce), xc_ccp_aad(NULL, &c, an, aad);
		xc_ccp_decrypt(NULL, &c, n, ours, back);
		CHECK(xc_ccp_verify(NULL, &c, tag) == CCMODE_INTEGRITY_FAILURE, "ChaCha20-Poly1305 forged tag accepted");
		/* a message dropped halfway: after reset only setnonce goes on, and the next message is as if it were the first */
		xc_ccp_reset(NULL, &c), xc_ccp_setnonce(NULL, &c, nonce), xc_ccp_aad(NULL, &c, an, aad);
		if (n > 0) xc_ccp_encrypt(NULL, &c, rndn(n) + 1, pt, back);
		xc_ccp_reset(NULL, &c);
		CHECK(xc_ccp_encrypt(NULL, &c, 1, pt, back) != 0, "ChaCha20-Poly1305 encrypt after reset without a nonce");
		CHECK(xc_ccp_aad(NULL, &c, 1, aad) != 0, "ChaCha20-Poly1305 aad after reset without a nonce");
		uint8_t t2[16];
		CHECK(xc_ccp_finalize(NULL, &c, t2) != 0, "ChaCha20-Poly1305 finalize after reset without a nonce");
		CHECK(xc_ccp_verify(NULL, &c, ttag) == CCERR_CALL_SEQUENCE, "ChaCha20-Poly1305 verify after reset without a nonce");
		xc_ccp_reset(NULL, &c), xc_ccp_setnonce(NULL, &c, nonce), xc_ccp_aad(NULL, &c, an, aad);
		xc_ccp_encrypt(NULL, &c, n, pt, back);
		CHECK(xc_ccp_finalize(NULL, &c, t2) == 0 && memcmp(back, theirs, n) == 0 && memcmp(t2, ttag, 16) == 0,
		    "ChaCha20-Poly1305 message after a dropped one");
		/* a message ends once */
		CHECK(xc_ccp_finalize(NULL, &c, t2) == CCERR_CALL_SEQUENCE, "ChaCha20-Poly1305 finalized twice");
		CHECK(xc_ccp_verify(NULL, &c, ttag) == CCERR_CALL_SEQUENCE, "ChaCha20-Poly1305 verify after finalize");
		CHECK(xc_ccp_encrypt(NULL, &c, 1, pt, back) != 0, "ChaCha20-Poly1305 encrypt after finalize");
	}
}

static void
test_rsa(void)
{
	static const struct { const char *md; const struct ccdigest_info *(*di)(void); const unsigned char *oid; } h[] = {
		{ "SHA1", xc_sha1_di, CC_DIGEST_OID_SHA1 }, { "SHA256", xc_sha256_di, CC_DIGEST_OID_SHA256 },
		{ "SHA384", xc_sha384_di, CC_DIGEST_OID_SHA384 }, { "SHA512", xc_sha512_di, CC_DIGEST_OID_SHA512 },
	};
	int sizes[] = { 1024, 2048, 3072, 4096 };
	for (int s = 0; s < 4; s++) {
		EVP_PKEY *pk = EVP_RSA_gen((unsigned)sizes[s]);
		BIGNUM *bn = NULL, *be = NULL;
		uint8_t mod[512], ex[8], dig[64], sig[512], msg[100];
		EVP_PKEY_get_bn_param(pk, OSSL_PKEY_PARAM_RSA_N, &bn);
		EVP_PKEY_get_bn_param(pk, OSSL_PKEY_PARAM_RSA_E, &be);
		int mn = BN_bn2bin(bn, mod), en = BN_bn2bin(be, ex);
		/* the kernel's context: room for 4096 bits (libkern/crypto/rsa.h) */
		ccrsa_pub_ctx_decl(ccn_sizeof(4096), key);
		ccrsa_ctx_n(key) = ccn_nof(4096);
		CHECK(xc_rsa_make_pub(key, (size_t)en, ex, (size_t)mn, mod) == 0, "make_pub %d", sizes[s]);
		for (int j = 0; j < 4; j++) {
			const struct ccdigest_info *di = h[j].di();
			size_t sl = sizeof(sig);
			bool valid;
			fill(msg, sizeof(msg));
			xc_digest(di, sizeof(msg), msg, dig);
			EVP_PKEY_CTX *c = EVP_PKEY_CTX_new(pk, NULL);
			EVP_PKEY_sign_init(c);
			EVP_PKEY_CTX_set_rsa_padding(c, RSA_PKCS1_PADDING);
			EVP_PKEY_CTX_set_signature_md(c, EVP_get_digestbyname(h[j].md));
			EVP_PKEY_sign(c, sig, &sl, dig, di->output_size);
			EVP_PKEY_CTX_free(c);
			CHECK(xc_rsa_verify_pkcs1v15(key, h[j].oid, di->output_size, dig, sl, sig, &valid) == 0 && valid,
			    "RSA-%d %s valid signature", sizes[s], h[j].md);
			sig[rndn(sl)] ^= 1;
			CHECK(xc_rsa_verify_pkcs1v15(key, h[j].oid, di->output_size, dig, sl, sig, &valid) == 0 && !valid,
			    "RSA-%d %s forged signature accepted", sizes[s], h[j].md);
			sig[0] ^= 0, dig[0] ^= 1;
		}
		BN_free(bn), BN_free(be), EVP_PKEY_free(pk);
	}
	/* an object identifier that is no digest's, shorter than any: refused, and read no further than its own length */
	{
		ccrsa_pub_ctx_decl(ccn_sizeof(4096), key);
		uint8_t *oid = malloc(5), dig[32] = {0}, sig[512] = {0};
		bool valid = true;
		memcpy(oid, (uint8_t[]){ 0x06, 0x03, 0x2b, 0x0e, 0x03 }, 5);
		ccrsa_ctx_n(key) = ccn_nof(4096);
		CHECK(xc_rsa_verify_pkcs1v15(key, oid, 32, dig, 512, sig, &valid) == CCRSA_INVALID_INPUT && !valid,
		    "RSA with an unknown short OID");
		oid[0] = 0x04;
		CHECK(xc_rsa_verify_pkcs1v15(key, oid, 32, dig, 512, sig, &valid) == CCRSA_INVALID_INPUT, "RSA with no OID tag");
		free(oid);
	}
	CHECK(frees_wrong == 0, "%d frees named another size than their allocation", frees_wrong);
}

static void
test_rng(void)
{
	struct ccrng_state *r = xc_rng(NULL);
	uint64_t v, counts[10] = {0};
	for (int i = 0; i < 100000; i++) {
		CHECK(xc_rng_uniform(r, 10, &v) == 0 && v < 10, "uniform");
		counts[v]++;
	}
	for (int i = 0; i < 10; i++) {
		CHECK(counts[i] > 9000 && counts[i] < 11000, "uniform bucket %d: %llu", i, (unsigned long long)counts[i]);
	}
	/* kmem's generator: uniform, any lengths, and two of them differ */
	_Alignas(16) uint8_t a[256], b[256];
	CHECK(xc_kmem_rng_size() <= sizeof(a), "kmem generator size");
	unsigned long calls = host_calls;
	xc_kmem_rng_init(a), xc_kmem_rng_init(b);
	CHECK(host_calls == calls + 2, "kmem generator keyed from the host at init");
	calls = host_calls;
	memset(counts, 0, sizeof(counts));
	for (int i = 0; i < 100000; i++) {
		CHECK(xc_rng_uniform((struct ccrng_state *)a, 10, &v) == 0 && v < 10, "kmem uniform");
		counts[v]++;
	}
	for (int i = 0; i < 10; i++) {
		CHECK(counts[i] > 9000 && counts[i] < 11000, "kmem uniform bucket %d: %llu", i, (unsigned long long)counts[i]);
	}
	uint8_t x[1000], y[1000];
	for (size_t off = 0; off < sizeof(x);) {
		size_t m = 1 + rndn(sizeof(x) - off);
		ccrng_generate((struct ccrng_state *)a, m, x + off);
		off += m;
	}
	ccrng_generate((struct ccrng_state *)b, sizeof(y), y);
	CHECK(host_calls == calls, "kmem generator asked the host %lu times after init", host_calls - calls);
	CHECK(memcmp(x, y, sizeof(x)) != 0, "two kmem generators gave the same bytes");
	size_t ones = 0;
	for (size_t i = 0; i < sizeof(x); i++) ones += (size_t)__builtin_popcount(x[i]);
	CHECK(ones > 3700 && ones < 4300, "kmem generator bits: %zu ones of 8000", ones);
}

int
main(void)
{
	OSSL_PROVIDER *legacy = OSSL_PROVIDER_load(NULL, "legacy"), *dflt = OSSL_PROVIDER_load(NULL, "default");
	xc_digest_setup();
	int st = xc_selftest();
	CHECK(st == 0, "self test %d", st);
	test_digests();
	test_aes();
	test_gcm();
	test_des();
	test_cts3();
	test_chachapoly();
	test_rsa();
	test_rng();
	OSSL_PROVIDER_unload(legacy), OSSL_PROVIDER_unload(dflt);
	printf("%s: %d failure(s)\n", failures ? "FAILED" : "PASSED", failures);
	return failures != 0;
}

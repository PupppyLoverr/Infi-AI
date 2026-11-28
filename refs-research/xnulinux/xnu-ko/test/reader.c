// SPDX-License-Identifier: APSL-2.0
/*
 * What XNU reads of the boot data xnu.ko hands it, written from XNU's readers (xnu-12377.121.6) and sharing nothing
 * with the module's writers (adt.c, build_boot): test.c reads back with this what the module wrote.
 *
 * The device tree as pexpert/gen/device_tree.c walks it: a node is {u32 nProperties, u32 nChildren}, its properties,
 * then its children; a property is {char name[32], u32 length}, then the value, padded to 4 bytes. XNU takes length
 * whole (it does not mask the top bit that a bootloader's own tree uses as a flag), finds a property by strcmp on the
 * name (the first match) and a child by its "name" property. boot_args is pexpert/pexpert/arm64/boot.h; a boot-arg
 * is found as pexpert/gen/bootargs.c finds it.
 */
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "reader.h"

struct walk { const unsigned char *base; unsigned long len, at; char *err; unsigned long errlen; int depth; };

static int fail(struct walk *w, const char *fmt, ...)
{
	va_list ap;

	if (w->err && !w->err[0]) { va_start(ap, fmt); vsnprintf(w->err, w->errlen, fmt, ap); va_end(ap); }
	return 0;
}
static uint32_t le32(const unsigned char *p) { return p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24; }

static struct xr_node *node(struct walk *w, struct xr_node *parent)
{
	unsigned long start = w->at;
	struct xr_node *n, **link;

	if (++w->depth > 16) { fail(w, "node at %#lx: more than 16 levels deep", start); return 0; }
	if (w->len - w->at < 8) { fail(w, "node at %#lx: its header is past the tree's end (%#lx)", start, w->len); return 0; }
	if (!(n = calloc(1, sizeof(*n)))) return 0;
	n->parent = parent; n->nprops = le32(w->base + w->at); n->nchildren = le32(w->base + w->at + 4); w->at += 8;
	if (!n->nprops) { fail(w, "node at %#lx: no properties (XNU reads a node without any as the end of the tree)", start); goto bad; }
	if (n->nprops > 4096 || n->nchildren > 4096) { fail(w, "node at %#lx: %u properties, %u children", start, n->nprops, n->nchildren); goto bad; }
	if (!(n->props = calloc(n->nprops, sizeof(*n->props)))) goto bad;
	for (unsigned i = 0; i < n->nprops; i++) {
		struct xr_prop *p = &n->props[i];
		const unsigned char *h = w->base + w->at;
		unsigned long nlen, room;

		if (w->len - w->at < 36) { fail(w, "node at %#lx, property %u: its header is past the tree's end", start, i); goto bad; }
		for (nlen = 0; nlen < 32 && h[nlen]; nlen++) ;
		if (nlen == 32) { fail(w, "node at %#lx, property %u: the name is not terminated in its 32 bytes", start, i); goto bad; }
		if (!nlen) { fail(w, "node at %#lx, property %u: an empty name", start, i); goto bad; }
		for (unsigned long k = nlen; k < 32; k++)
			if (h[k]) { fail(w, "node at %#lx, property \"%.31s\": bytes after the name's terminator", start, (const char *)h); goto bad; }
		memcpy(p->name, h, nlen);
		p->len = le32(h + 32); p->val = h + 36; w->at += 36;
		room = w->len - w->at;
		if (p->len > room || ((p->len + 3UL) & ~3UL) > room) {
			fail(w, "node at %#lx, property \"%s\": length %#x runs past the tree's end (%#lx left)", start, p->name, p->len, room);
			goto bad;
		}
		for (unsigned long k = p->len; k < ((p->len + 3UL) & ~3UL); k++)
			if (p->val[k]) { fail(w, "node at %#lx, property \"%s\": its padding is not zero", start, p->name); goto bad; }
		w->at += (p->len + 3UL) & ~3UL;
	}
	link = &n->child;
	for (unsigned i = 0; i < n->nchildren; i++) {
		if (!(*link = node(w, n))) goto bad;
		link = &(*link)->next;
	}
	w->depth--;
	return n;
bad:
	xr_dt_free(n);
	return 0;
}

struct xr_node *xr_dt_parse(const void *dt, unsigned long len, char *err, unsigned long errlen)
{
	struct walk w = { dt, len, 0, err, errlen, 0 };
	struct xr_node *root;

	if (err && errlen) err[0] = 0;
	if (!(root = node(&w, 0))) { fail(&w, "out of memory"); return 0; }
	if (w.at != len) {
		fail(&w, "the tree ends at %#lx, its length is given as %#lx", w.at, len);
		xr_dt_free(root);
		return 0;
	}
	return root;
}

void xr_dt_free(struct xr_node *n)
{
	if (!n) return;
	for (struct xr_node *c = n->child, *next; c; c = next) { next = c->next; xr_dt_free(c); }
	free(n->props);
	free(n);
}

const struct xr_prop *xr_prop(const struct xr_node *n, const char *name)
{
	for (unsigned i = 0; n && i < n->nprops; i++)
		if (!strcmp(n->props[i].name, name)) return &n->props[i];
	return 0;
}
unsigned xr_prop_count(const struct xr_node *n, const char *name)
{
	unsigned c = 0;

	for (unsigned i = 0; n && i < n->nprops; i++) c += !strcmp(n->props[i].name, name);
	return c;
}
int xr_is_str(const struct xr_prop *p) { return p && p->len && !p->val[p->len - 1] && strlen((const char *)p->val) == p->len - 1; }
int xr_str_eq(const struct xr_prop *p, const char *s) { return xr_is_str(p) && !strcmp((const char *)p->val, s); }
const char *xr_name(const struct xr_node *n)
{
	const struct xr_prop *p = xr_prop(n, "name");

	return xr_is_str(p) ? (const char *)p->val : 0;
}
uint32_t xr_u32(const struct xr_prop *p, unsigned i) { return le32(p->val + 4 * i); }
uint64_t xr_u64(const struct xr_prop *p, unsigned i) { return xr_u32(p, 2 * i) | (uint64_t)xr_u32(p, 2 * i + 1) << 32; }

/* each component is the "name" of a child, the first that has it (FindChild) */
const struct xr_node *xr_lookup(const struct xr_node *root, const char *path)
{
	const struct xr_node *n = root;

	while (n && *path) {
		const char *e;
		const struct xr_node *c;

		while (*path == '/') path++;
		if (!*path) break;
		for (e = path; *e && *e != '/'; e++) ;
		for (c = n->child; c; c = c->next) {
			const char *name = xr_name(c);

			if (name && strlen(name) == (unsigned long)(e - path) && !memcmp(name, path, e - path)) break;
		}
		n = c; path = e;
	}
	return n;
}

/* the first node, parents before children, with a property `prop` whose value starts with the C string `value` (find_entry) */
const struct xr_node *xr_find(const struct xr_node *n, const char *prop, const char *value)
{
	const struct xr_node *f;

	if (!n) return 0;
	for (unsigned i = 0; i < n->nprops; i++)
		if (!strcmp(n->props[i].name, prop) && n->props[i].len > strlen(value) && !strcmp((const char *)n->props[i].val, value))
			return n;
	for (const struct xr_node *c = n->child; c; c = c->next)
		if ((f = xr_find(c, prop, value))) return f;
	return 0;
}

uint32_t xr_ba32(const void *ba, unsigned off) { return le32((const unsigned char *)ba + off); }
uint16_t xr_ba16(const void *ba, unsigned off) { const unsigned char *p = (const unsigned char *)ba + off; return p[0] | p[1] << 8; }
uint64_t xr_ba64(const void *ba, unsigned off) { return xr_ba32(ba, off) | (uint64_t)xr_ba32(ba, off + 4) << 32; }

static int sep(char c) { return c == ' ' || c == '\t' || !c; }

int xr_bootarg(const char *line, const char *name, char *val, unsigned long max)
{
	const char *a = line;
	int found = 0;

	while (*a && sep(*a)) a++;
	while (*a) {
		const char *e = a, *v = "1";
		unsigned long n, vl = 1;

		while (!sep(*e) && *e != '=') e++;
		n = e - a;
		if (n == strlen(name) && !strncmp(a, name, n)) {
			if (*e == '=') { v = e + 1; for (vl = 0; !sep(v[vl]); vl++) ; }
			if (vl > max - 1) vl = max - 1;
			memcpy(val, v, vl); val[vl] = 0; found = 1;
			if (*a == '-') break;
		}
		while (!sep(*a)) a++;
		while (*a && sep(*a)) a++;
	}
	return found;
}

static uint32_t rotr(uint32_t x, unsigned n) { return x >> n | x << (32 - n); }

void xr_sha256(const void *data, unsigned long len, unsigned char out[32])
{
	uint32_t h[8] = { 0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19 }, k[64];
	unsigned long total = (len + 9 + 63) & ~63UL;
	unsigned char *m = calloc(1, total);
	unsigned primes = 0;

	for (unsigned n = 2; primes < 64; n++) {	/* the constants: the fractional parts of the cube roots of the first 64 primes */
		unsigned d;
		double r, f;

		for (d = 2; d * d <= n && n % d; d++) ;
		if (d * d <= n) continue;
		for (r = n, d = 0; d < 80; d++) r = (2 * r + n / (r * r)) / 3;	/* Newton's cube root */
		f = r - (unsigned)r;
		k[primes++] = (uint32_t)(f * 4294967296.0);
	}
	memcpy(m, data, len); m[len] = 0x80;
	for (unsigned i = 0; i < 8; i++) m[total - 1 - i] = (unsigned char)((uint64_t)len * 8 >> (8 * i));
	for (unsigned long o = 0; o < total; o += 64) {
		uint32_t w[64], s[8];

		for (unsigned i = 0; i < 16; i++) w[i] = (uint32_t)m[o + 4 * i] << 24 | m[o + 4 * i + 1] << 16 | m[o + 4 * i + 2] << 8 | m[o + 4 * i + 3];
		for (unsigned i = 16; i < 64; i++)
			w[i] = w[i - 16] + (rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ w[i - 15] >> 3) + w[i - 7] +
			    (rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ w[i - 2] >> 10);
		memcpy(s, h, sizeof(s));
		for (unsigned i = 0; i < 64; i++) {
			uint32_t t1 = s[7] + (rotr(s[4], 6) ^ rotr(s[4], 11) ^ rotr(s[4], 25)) + ((s[4] & s[5]) ^ (~s[4] & s[6])) + k[i] + w[i];
			uint32_t t2 = (rotr(s[0], 2) ^ rotr(s[0], 13) ^ rotr(s[0], 22)) + ((s[0] & s[1]) ^ (s[0] & s[2]) ^ (s[1] & s[2]));

			memmove(s + 1, s, 7 * sizeof(s[0])); s[4] += t1; s[0] = t1 + t2;
		}
		for (unsigned i = 0; i < 8; i++) h[i] += s[i];
	}
	for (unsigned i = 0; i < 32; i++) out[i] = (unsigned char)(h[i / 4] >> (24 - 8 * (i % 4)));
	free(m);
}

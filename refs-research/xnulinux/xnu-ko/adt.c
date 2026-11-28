// SPDX-License-Identifier: APSL-2.0
// ADT: node = {u32 nProperties, u32 nChildren}, then properties
// {char name[32]; u32 length; value padded to 4}, then children.
#include "xrt.h"
#include "adt.h"

void adt_init(struct adt *a, void *buf, unsigned long cap)
{
	a->buf = a->p = buf; a->end = a->buf + cap; a->err = 0;
}

static void put(struct adt *a, const void *v, unsigned long n)
{
	if (a->p + n > a->end) { a->err = 1; return; }
	x_memcpy(a->p, v, n); a->p += n;
}

void adt_node(struct adt *a, unsigned nprops, unsigned nchildren)
{
	unsigned h[2] = { nprops, nchildren };
	put(a, h, sizeof(h));
}

void adt_prop(struct adt *a, const char *name, const void *val, unsigned len)
{
	char n[32] = { 0 };
	unsigned long l = x_strlen(name);
	static const unsigned char zero[4];
	x_memcpy(n, name, l < 31 ? l : 31);
	put(a, n, 32);
	put(a, &len, 4);
	put(a, val, len);
	put(a, zero, (4 - (len & 3)) & 3);
}

void adt_prop_str(struct adt *a, const char *name, const char *s) { adt_prop(a, name, s, x_strlen(s) + 1); }
void adt_prop_u32(struct adt *a, const char *name, unsigned v) { adt_prop(a, name, &v, 4); }
void adt_prop_u64(struct adt *a, const char *name, unsigned long long v) { adt_prop(a, name, &v, 8); }

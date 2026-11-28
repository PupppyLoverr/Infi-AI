// SPDX-License-Identifier: APSL-2.0
// Mach-O loading and threaded-rebase for the XNU image (APSL side).
#include "xrt.h"
#define ENOEXEC 8
#define ENOMEM 12
#include "macho.h"

static int lc_bounds_ok(const unsigned char *base, unsigned long size,
			u32_t ncmds, u32_t sizeofcmds)
{
	if (size < sizeof(struct mh64))
		return 0;
	if ((unsigned long)sizeofcmds > size - sizeof(struct mh64))
		return 0;
	const unsigned char *p = base + sizeof(struct mh64);
	const unsigned char *end = p + sizeofcmds;
	for (u32_t i = 0; i < ncmds; i++) {
		if ((unsigned long)(end - p) < sizeof(struct lc))
			return 0;
		const struct lc *l = (const void *)p;
		if (l->cmdsize < sizeof(struct lc) || l->cmdsize > (unsigned long)(end - p))
			return 0;
		p += l->cmdsize;
	}
	return 1;
}

static int cstr_bounded_eq(const char *s, unsigned long max_len, const char *want)
{
	unsigned long i = 0;
	while (i < max_len) {
		if (s[i] != want[i])
			return 0;
		if (s[i] == '\0')
			return 1;
		i++;
	}
	return 0;
}

#define LC_OK(l, end) \
	((const char *)(end) - (const char *)(l) >= (long)sizeof(struct lc) && \
	 (l)->cmdsize >= sizeof(struct lc) && \
	 (l)->cmdsize <= (u64_t)((const char *)(end) - (const char *)(l)))

#define FOR_EACH_LC(img, l) \
	for (u32_t _lci = 0, _ncmds = ((const struct mh64 *)(img)->file)->ncmds; _lci < _ncmds; _lci = _ncmds) \
	for (const struct lc *l = (const void *)((img)->file + sizeof(struct mh64)), *_e = (const void *)((img)->file + sizeof(struct mh64) + ((const struct mh64 *)(img)->file)->sizeofcmds); \
	     _lci < _ncmds && LC_OK(l, _e); _lci++, l = (const void *)((const char *)l + l->cmdsize))

int macho_load(struct xnu_image *img, const void *file, unsigned long size, unsigned long extra)
{
	const struct mh64 *mh = file;
	int err;
	(void)err;

	img->file = file; img->file_size = size;
	if (!file || size < sizeof(*mh) || mh->magic != MH_MAGIC_64)
		return -ENOEXEC;
	if (!lc_bounds_ok(img->file, size, mh->ncmds, mh->sizeofcmds))
		return -ENOEXEC;
	img->vmlow = ~0ULL; img->vmhigh = 0;
	FOR_EACH_LC(img, l) {
		if (l->cmd == LC_SEGMENT_64) {
			if (l->cmdsize < sizeof(struct seg64))
				return -ENOEXEC;
			const struct seg64 *s = (const void *)l;
			if (s->nsects > (l->cmdsize - sizeof(struct seg64)) / sizeof(struct sect64))
				return -ENOEXEC;
			if (!s->vmsize || !x_strncmp(s->segname, "__PAGEZERO", 16))
				continue;
			if (s->vmaddr + s->vmsize < s->vmaddr)
				return -ENOEXEC;
			if (s->filesize > s->vmsize || s->fileoff > size || s->filesize > size - s->fileoff)
				return -ENOEXEC;
			if (s->vmaddr < img->vmlow) img->vmlow = s->vmaddr;
			if (s->vmaddr + s->vmsize > img->vmhigh) img->vmhigh = s->vmaddr + s->vmsize;
			const struct sect64 *sc = (const void *)(s + 1);
			for (u32_t i = 0; i < s->nsects; i++)
				if (!x_strncmp(sc[i].sectname, "__thread_starts", 16)) {
					img->thread_starts = sc[i].addr;
					img->thread_starts_size = sc[i].size;
				}
		} else if (l->cmd == LC_SYMTAB) {
			if (l->cmdsize < sizeof(struct symtab_lc))
				return -ENOEXEC;
			img->symtab = (const void *)l;
		} else if (l->cmd == LC_UNIXTHREAD) {
			/* flavor, count, then arm_thread_state64: x[29], fp, lr, sp, pc */
			if (l->cmdsize < sizeof(struct lc) + (2 + 2 * 32) * sizeof(u32_t) + sizeof(u64_t))
				return -ENOEXEC;
			const u32_t *t = (const void *)(l + 1);
			img->entry = *(const u64_t *)(t + 2 + 2 * 32);
		}
	}
	if (img->vmhigh <= img->vmlow)
		return -ENOEXEC;
	img->filetype = mh->filetype;
	if (mh->filetype == MH_FILESET) {
		/*
		 * A kernel collection: a flat image (file offset = vmaddr - base) whose pointers are chained fixups that XNU's
		 * arm_init applies itself (kernel_collection_slide). The kernel's symbols are in its own entry's LC_SYMTAB.
		 */
		img->symtab = 0;
		FOR_EACH_LC(img, l) {
			if (l->cmd != LC_FILESET_ENTRY) continue;
			if (l->cmdsize < sizeof(struct fileset_entry_lc)) return -ENOEXEC;
			const struct fileset_entry_lc *fe = (const void *)l;
			if (fe->entry_id >= fe->cmdsize ||
			    !cstr_bounded_eq((const char *)fe + fe->entry_id, fe->cmdsize - fe->entry_id, "com.apple.kernel"))
				continue;
			if (fe->fileoff > size || size - fe->fileoff < sizeof(struct mh64))
				return -ENOEXEC;
			const struct mh64 *kh = (const void *)((const char *)file + fe->fileoff);
			if (kh->magic != MH_MAGIC_64 ||
			    !lc_bounds_ok((const unsigned char *)kh, size - fe->fileoff, kh->ncmds, kh->sizeofcmds))
				return -ENOEXEC;
			img->kernel_mh = fe->vmaddr;
			const struct lc *kl = (const void *)(kh + 1);
			for (u32_t i = 0; i < kh->ncmds; i++, kl = (const void *)((const char *)kl + kl->cmdsize)) {
				if (kl->cmd == LC_SYMTAB) {
					if (kl->cmdsize < sizeof(struct symtab_lc))
						return -ENOEXEC;
					img->symtab = (const void *)kl;
				}
			}
		}
		if (!img->kernel_mh || !img->symtab)
			return -ENOEXEC;
	}
	if (img->symtab) {
		const struct symtab_lc *st = img->symtab;
		if ((u64_t)st->symoff > size || (u64_t)st->nsyms > (size - st->symoff) / sizeof(struct nlist64) ||
		    (u64_t)st->stroff > size || (u64_t)st->strsize > size - st->stroff)
			return -ENOEXEC;
	}
	if (extra == ~0UL)		/* parse only: the caller places the image (macho_copy) */
		return 0;
	if ((img->vmhigh - img->vmlow) + extra < (img->vmhigh - img->vmlow))
		return -ENOMEM;
	err = fki_region_alloc(img->vmhigh - img->vmlow + extra, &img->region);
	if (err)
		return err;
	img->slide = (u64_t)img->region.va - img->vmlow;
	return macho_copy(img, img->region.va);
}

/* Copy the segments to dst, which is where vmlow will live. */
int macho_copy(struct xnu_image *img, void *dst)
{
	u64_t img_span = img->vmhigh - img->vmlow;
	FOR_EACH_LC(img, l) {
		if (l->cmd != LC_SEGMENT_64) continue;
		const struct seg64 *s = (const void *)l;
		if (!s->vmsize || !s->filesize || !x_strncmp(s->segname, "__PAGEZERO", 16)) continue;
		if (s->fileoff > img->file_size || s->filesize > img->file_size - s->fileoff) return -ENOEXEC;
		if (s->vmaddr < img->vmlow || (s->vmaddr - img->vmlow) > img_span ||
		    s->filesize > img_span - (s->vmaddr - img->vmlow))
			return -ENOEXEC;
		x_memcpy((char *)dst + (s->vmaddr - img->vmlow), img->file + s->fileoff, s->filesize);
	}
	return 0;
}

static inline u64_t sign(u64_t v, unsigned key, u64_t disc)
{
	switch (key) {
	case 0: asm volatile(".arch_extension pauth\n\tpacia %0, %1" : "+r"(v) : "r"(disc)); break;
	case 1: asm volatile(".arch_extension pauth\n\tpacib %0, %1" : "+r"(v) : "r"(disc)); break;
	case 2: asm volatile(".arch_extension pauth\n\tpacda %0, %1" : "+r"(v) : "r"(disc)); break;
	default: asm volatile(".arch_extension pauth\n\tpacdb %0, %1" : "+r"(v) : "r"(disc)); break;
	}
	return v;
}

/* Runs inside fki_enter() with the XNU kernel keys loaded. */
int macho_rebase(struct xnu_image *img)
{
	u64_t mh_vm = 0;
	FOR_EACH_LC(img, l) {
		if (l->cmd == LC_SEGMENT_64 && !x_strncmp(((const struct seg64 *)l)->segname, "__TEXT", 16) &&
		    ((const struct seg64 *)l)->fileoff == 0)
			mh_vm = ((const struct seg64 *)l)->vmaddr;
	}
	if (!img->thread_starts || !mh_vm)
		return -ENOEXEC;
	/*
	 * The section and every fixup inside the loaded image: the section header's address and the offsets in its list
	 * were followed wherever they pointed, read and written here with XNU's keys loaded and interrupts off.
	 */
	u64_t ts_va, lo = img->vmlow + img->slide, hi = img->vmhigh + img->slide;
	if (img->thread_starts_size < 4 || !macho_va_to_loaded(img, img->thread_starts, img->thread_starts_size, &ts_va))
		return -ENOEXEC;
	u32_t *ts = (u32_t *)ts_va;
	u32_t *te = ts + img->thread_starts_size / 4;
	u64_t step = (ts[0] & 1) ? 8 : 4, n = 0;
	for (u32_t *o = ts + 1; o < te && *o != 0xffffffff; o++) {
		u64_t addr = mh_vm + img->slide + *o, delta;
		do {
			if (addr < lo || addr > hi - 8)
				return -ENOEXEC;
			u64_t v = *(u64_t *)addr;
			if (!(v & (1ULL << 62))) {
				if (v & (1ULL << 63)) {
					u64_t nv = (v & 0xffffffffULL) + mh_vm + img->slide;
					u64_t disc = (v >> 32) & 0xffff;
					if (v & (1ULL << 48))
						disc = disc ? ((addr & 0x0000ffffffffffffULL) | (disc << 48)) : addr;
					*(u64_t *)addr = sign(nv, (v >> 49) & 3, disc);
				} else {
					u64_t top8 = v & 0x0007F80000000000ULL, b43 = v & 0x000007FFFFFFFFFFULL;
					u64_t t = (top8 << 13) | (((long long)(b43 << 21) >> 21) & 0x00FFFFFFFFFFFFFFULL);
					*(u64_t *)addr = t + img->slide;
				}
				n++;
			}
			delta = ((v & ~(1ULL << 62)) & 0x3FF8000000000000ULL) >> 51;
			addr += delta * step;
		} while (delta);
	}
	return (int)n;
}

int macho_va_to_loaded(const struct xnu_image *img, u64_t unslid, u64_t len, u64_t *out_va)
{
	if (!img || !out_va || img->vmhigh < img->vmlow)
		return 0;
	if (unslid < img->vmlow || unslid > img->vmhigh)
		return 0;
	u64_t off = unslid - img->vmlow;
	u64_t span = img->vmhigh - img->vmlow;
	if (len > span - off)
		return 0;
	u64_t base = img->vmlow + img->slide;
	if (base + off < base)
		return 0;
	u64_t va = base + off;
	if (va + len < va)
		return 0;
	*out_va = va;
	return 1;
}

u64_t macho_sym(struct xnu_image *img, const char *name)
{
	const struct symtab_lc *st = img->symtab;
	if (!st) return 0;
	const struct nlist64 *nl = (const void *)(img->file + st->symoff);
	const char *strs = (const char *)img->file + st->stroff;
	for (u32_t i = 0; i < st->nsyms; i++) {
		if ((nl[i].n_type & 0x0e) != 0x0e || nl[i].n_strx >= st->strsize) continue;	/* N_SECT */
		if (cstr_bounded_eq(strs + nl[i].n_strx, st->strsize - nl[i].n_strx, name)) {
			u64_t va;
			if (!macho_va_to_loaded(img, nl[i].n_value, 1, &va))
				return 0;
			return va;
		}
	}
	return 0;
}

/* As iBoot does for a static kernelcache: slide the load commands of the in-memory Mach-O header. */
int macho_slide_header(struct xnu_image *img, u64_t *mh_unslid)
{
	u64_t mh_vm = 0;
	FOR_EACH_LC(img, l) {
		if (l->cmd == LC_SEGMENT_64 && !x_strncmp(((const struct seg64 *)l)->segname, "__TEXT", 16) &&
		    ((const struct seg64 *)l)->fileoff == 0)
			mh_vm = ((const struct seg64 *)l)->vmaddr;
	}
	u64_t mh_va;
	if (!mh_vm || !macho_va_to_loaded(img, mh_vm, sizeof(struct mh64), &mh_va))
		return -ENOEXEC;
	*mh_unslid = mh_vm;
	unsigned char *mh = (unsigned char *)mh_va;
	const struct mh64 *h = (const void *)mh;
	/*
	 * The commands as loaded, which are not the file's that macho_load checked when a segment lies over them or a
	 * fixup was in them: they stay inside the image, and a segment's sections inside its command.
	 */
	if (h->sizeofcmds > img->vmhigh - mh_vm - sizeof(struct mh64))
		return -ENOEXEC;
	struct lc *l = (void *)(mh + sizeof(struct mh64)), *e = (void *)(mh + sizeof(struct mh64) + h->sizeofcmds);
	int n = 0;
	for (u32_t ci = 0; ci < h->ncmds && LC_OK(l, e); ci++, l = (void *)((char *)l + l->cmdsize)) {
		if (l->cmd != LC_SEGMENT_64) continue;
		struct seg64 *sg = (void *)l;
		if (l->cmdsize < sizeof(struct seg64) || sg->nsects > (l->cmdsize - sizeof(struct seg64)) / sizeof(struct sect64))
			return -ENOEXEC;
		if (!sg->vmsize) continue;
		sg->vmaddr += img->slide;
		struct sect64 *sc = (void *)(sg + 1);
		for (u32_t i = 0; i < sg->nsects; i++) sc[i].addr += img->slide;
		n++;
	}
	return n;
}

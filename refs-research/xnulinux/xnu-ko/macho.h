// SPDX-License-Identifier: APSL-2.0
/* Minimal Mach-O definitions (written from the public format, no Apple headers). */
#pragma once
#include "../fki-ko/fki.h"
typedef unsigned int u32_t;
typedef unsigned long long u64_t;

#define MH_MAGIC_64 0xfeedfacf
#define LC_SEGMENT_64 0x19
#define LC_SYMTAB 0x2
#define LC_UNIXTHREAD 0x5
#define LC_FILESET_ENTRY 0x80000035
#define MH_FILESET 12

struct mh64 { u32_t magic, cputype, cpusubtype, filetype, ncmds, sizeofcmds, flags, reserved; };
struct lc { u32_t cmd, cmdsize; };
struct seg64 { u32_t cmd, cmdsize; char segname[16]; u64_t vmaddr, vmsize, fileoff, filesize; u32_t maxprot, initprot, nsects, flags; };
struct sect64 { char sectname[16], segname[16]; u64_t addr, size; u32_t offset, align, reloff, nreloc, flags, r1, r2, r3; };
struct symtab_lc { u32_t cmd, cmdsize, symoff, nsyms, stroff, strsize; };
struct nlist64 { u32_t n_strx; unsigned char n_type, n_sect; unsigned short n_desc; u64_t n_value; };

struct xnu_image {
	const unsigned char *file; unsigned long file_size;
	u64_t vmlow, vmhigh;		/* unslid */
	struct fki_region region;
	u64_t slide;			/* runtime = unslid + slide */
	const struct symtab_lc *symtab;
	u64_t thread_starts, thread_starts_size;	/* unslid vmaddr */
	u64_t entry;			/* unslid */
	u32_t filetype;			/* MH_EXECUTE kernel or MH_FILESET kernel collection */
	u64_t kernel_mh;		/* MH_FILESET: the com.apple.kernel entry's header (unslid vmaddr) */
};
struct fileset_entry_lc { u32_t cmd, cmdsize; u64_t vmaddr, fileoff; u32_t entry_id, reserved; };

int macho_load(struct xnu_image *img, const void *file, unsigned long size, unsigned long extra);
int macho_copy(struct xnu_image *img, void *dst);
int macho_rebase(struct xnu_image *img);
int macho_slide_header(struct xnu_image *img, u64_t *mh_unslid);
int macho_va_to_loaded(const struct xnu_image *img, u64_t unslid, u64_t len, u64_t *out_va);
u64_t macho_sym(struct xnu_image *img, const char *name);	/* runtime address or 0 */

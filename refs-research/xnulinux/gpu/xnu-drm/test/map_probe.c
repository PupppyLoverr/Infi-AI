// SPDX-License-Identifier: GPL-2.0-only
/*
 * In the hosted macOS: map a shared-memory blob over the DRM bridge and try each kind of access to it.
 *   clang -arch arm64 -O1 -include ../darwin/xnudrm_compat.h -I$W/mesa/include -I.. -I../darwin \
 *     -o map_probe map_probe.c ../darwin/xnudrm.c -framework IOKit -framework CoreFoundation
 * Usage: map_probe [bytes, default 147456]
 */
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <stdlib.h>
#include <fcntl.h>
#include <sys/mman.h>
#include "drm-uapi/drm.h"
#include "drm-uapi/virtgpu_drm.h"
#include "xf86drm.h"
int xnudrm_open(const char *path, int flags, ...);
void *xnudrm_mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off);
#undef os_dupfd_cloexec	/* xnudrm_compat.h names libxnudrm's; libxnudrm itself calls Mesa's (src/util/os_file.c), not linked here */
int os_dupfd_cloexec(int fd) { return fcntl(fd, F_DUPFD_CLOEXEC, 3); }
static sigjmp_buf jb;
static void h(int s, siginfo_t *si, void *u) { printf("  signal %d code %d addr %p\n", s, si->si_code, si->si_addr); siglongjmp(jb, 1); }
#define TRY(name, stmt) do { if (!sigsetjmp(jb, 1)) { stmt; printf("ok   %s\n", name); } else printf("FAIL %s\n", name); fflush(stdout); } while (0)
int main(int argc, char **argv)
{
	size_t size = argc > 1 ? strtoul(argv[1], 0, 0) : 147456;
	struct sigaction sa = { .sa_sigaction = h, .sa_flags = SA_SIGINFO | SA_NODEFER };
	sigaction(SIGBUS, &sa, 0); sigaction(SIGSEGV, &sa, 0);
	int fd = xnudrm_open("/dev/dri/renderD128", O_RDWR);
	struct drm_virtgpu_context_set_param sp = { VIRTGPU_CONTEXT_PARAM_CAPSET_ID, 4 };
	struct drm_virtgpu_context_init ci = { .num_params = 1, .ctx_set_params = (uintptr_t)&sp };
	printf("fd %d ctx %d\n", fd, drmIoctl(fd, DRM_IOCTL_VIRTGPU_CONTEXT_INIT, &ci));
	struct drm_virtgpu_resource_create_blob cb = { .blob_mem = VIRTGPU_BLOB_MEM_HOST3D, .blob_flags = VIRTGPU_BLOB_FLAG_USE_MAPPABLE, .size = size };
	printf("blob %d handle %u\n", drmIoctl(fd, DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB, &cb), cb.bo_handle);
	volatile uint8_t *p = xnudrm_mmap(0, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, (off_t)cb.bo_handle << 12);
	printf("map %p\n", (void *)p); fflush(stdout);
	if (p == MAP_FAILED) return 1;
	volatile uint32_t x; volatile uint64_t y;
	TRY("read byte 0", x = p[0]);
	TRY("read u32 0x40", x = *(volatile uint32_t *)(p + 0x40));
	TRY("write byte 0", p[0] = 1);
	TRY("write u32 0x40", *(volatile uint32_t *)(p + 0x40) = 7);
	TRY("atomic add u32 0x40", __atomic_fetch_add((uint32_t *)(p + 0x40), 1, __ATOMIC_SEQ_CST));
	TRY("write u64 0x48", *(volatile uint64_t *)(p + 0x48) = 9);
	TRY("unaligned u64 0x51", y = *(volatile uint64_t *)(p + 0x51));
	TRY("memset all", memset((void *)p, 0x5a, size));
	TRY("read last", x = p[size - 1]);
	(void)x; (void)y;
	return 0;
}

/* SPDX-License-Identifier: MIT */
/*
 * Force-included (pkg-config Cflags) into Mesa's Venus driver on macOS: the render node and its mappings and fences live
 * behind kexts/XnuDrm, so open/close/mmap/munmap/stat/poll on them go to libxnudrm. Everything else falls through to the
 * system's own calls.
 */
#ifndef XNUDRM_COMPAT_H
#define XNUDRM_COMPAT_H
#include <fcntl.h>
#include <poll.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#ifdef __cplusplus
extern "C" {
#endif
int xnudrm_open(const char *path, int flags, ...);
int xnudrm_close(int fd);
void *xnudrm_mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off);
int xnudrm_munmap(void *addr, size_t len);
int xnudrm_stat(const char *path, struct stat *st);
int xnudrm_poll(struct pollfd *fds, nfds_t n, int timeout);
int xnudrm_dupfd_cloexec(int fd);
#ifdef __cplusplus
}
#endif
#define open xnudrm_open
#define close xnudrm_close
#define mmap xnudrm_mmap
#define munmap xnudrm_munmap
#define stat(p, s) xnudrm_stat(p, s)
#define poll xnudrm_poll
#define os_dupfd_cloexec xnudrm_dupfd_cloexec
#endif

// SPDX-License-Identifier: APSL-2.0
// PureDarwin's IOMediaBSDClient waits on devfs_is_ready(), a call PureDarwin's kernel adds. This kernel has devfs up
// before IOKit matches the storage stack, as macOS does, so it is always ready here. Included with -include.
#ifdef __cplusplus
extern "C" {
#endif
static inline int
devfs_is_ready(void)
{
	return 1;
}
#ifdef __cplusplus
}
#endif

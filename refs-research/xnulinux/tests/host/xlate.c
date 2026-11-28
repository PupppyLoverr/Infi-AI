// SPDX-License-Identifier: GPL-2.0-only
/*
 * fki's /dev/xnu-hostcall translates a service's mappings into physical addresses (XNU_HOSTCALL_XLATE: what the DRM
 * bridge hands the hosted kernel to map). This checks the answers against the kernel's own, for both kinds of mapping:
 *   - ordinary memory (one page, locked): against /proc/self/pagemap
 *   - a device mapping (VM_IO | VM_PFNMAP): a PCI memory BAR mapped through sysfs, against the BAR's address in the
 *     device's `resource` file. The lookup for these is the one that differs by kernel version (fki_hostcall.c).
 * Needs root, fki.ko loaded and no other user of /dev/xnu-hostcall (xnudrmd). No hosted system is needed.
 *   cc -O2 -o xlate xlate.c && sudo ./xlate [/sys/bus/pci/devices/0000:BB:DD.F/resourceN]
 * Without an argument it takes the first PCI memory BAR that can be mapped (one no driver has claimed); with none the
 * device half is skipped and says so. Exit status 0: every answer right; 1: a wrong answer; 2: could not run.
 */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

struct xnu_hostcall_xlate { uint64_t va, len, out, max; };
#define XNU_HOSTCALL_XLATE _IOWR('X', 1, struct xnu_hostcall_xlate)

static int hc, bad;

static int xlate(void *va, uint64_t *pa)
{
	struct xnu_hostcall_xlate x = { (uintptr_t)va, 4096, (uintptr_t)pa, 1 };
	return ioctl(hc, XNU_HOSTCALL_XLATE, &x) ? -errno : 0;
}

static void check(const char *what, void *va, uint64_t want)
{
	uint64_t pa = 0;
	int r = xlate(va, &pa);
	int ok = r == 0 && pa == want;

	printf("%s: %s: XLATE %s %#llx, the kernel says %#llx\n", ok ? "ok" : "FAIL", what, r ? strerror(-r) : "gives",
	    (unsigned long long)pa, (unsigned long long)want);
	if (!ok) bad = 1;
}

/* map the first page of a sysfs PCI resource file; *start: the BAR's address (line N of the device's `resource`) */
static void *map_bar(const char *path, uint64_t *start)
{
	char res[512], line[128];
	const char *n = strrchr(path, '/');
	unsigned idx, i;
	uint64_t s, e, fl;
	void *va = MAP_FAILED;
	FILE *f;
	int fd;

	if (!n || sscanf(n, "/resource%u", &idx) != 1 || (size_t)(n - path) + 10 > sizeof(res)) return MAP_FAILED;
	memcpy(res, path, n - path);
	strcpy(res + (n - path), "/resource");
	if (!(f = fopen(res, "r"))) return MAP_FAILED;
	for (i = 0; i <= idx; i++) if (!fgets(line, sizeof(line), f)) { fclose(f); return MAP_FAILED; }
	fclose(f);
	if (sscanf(line, "%llx %llx %llx", (unsigned long long *)&s, (unsigned long long *)&e, (unsigned long long *)&fl) != 3) return MAP_FAILED;
	if (!s || !(fl & 0x200) || e - s + 1 < 4096) return MAP_FAILED;	/* IORESOURCE_MEM, at least a page */
	if ((fd = open(path, O_RDWR | O_SYNC)) < 0) return MAP_FAILED;
	va = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	close(fd);
	*start = s;
	return va;
}

int main(int argc, char **argv)
{
	uint64_t e = 0, start = 0;
	char path[512] = "";
	void *va = MAP_FAILED, *page;
	int pm;

	hc = open("/dev/xnu-hostcall", O_RDWR);
	if (hc < 0) { perror("/dev/xnu-hostcall (is fki.ko loaded, are you root?)"); return 2; }

	page = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_POPULATE, -1, 0);
	pm = open("/proc/self/pagemap", O_RDONLY);
	if (page == MAP_FAILED || mlock(page, 4096) || pm < 0) { perror("a locked page and /proc/self/pagemap"); return 2; }
	*(volatile char *)page = 1;
	if (pread(pm, &e, 8, ((uintptr_t)page / 4096) * 8) != 8 || !(e >> 63) || !(e & ((1ULL << 55) - 1))) {
		fprintf(stderr, "/proc/self/pagemap gives no page frame (not root?)\n"); return 2;
	}
	check("ordinary memory", page, (e & ((1ULL << 55) - 1)) << 12);

	if (argc > 1) {
		snprintf(path, sizeof(path), "%s", argv[1]);
		va = map_bar(path, &start);
		if (va == MAP_FAILED) { fprintf(stderr, "%s: cannot map it as a memory BAR (%s)\n", path, strerror(errno)); return 2; }
	} else {
		DIR *d = opendir("/sys/bus/pci/devices");
		struct dirent *de;
		while (d && va == MAP_FAILED && (de = readdir(d))) {
			if (de->d_name[0] == '.') continue;
			for (int i = 0; i < 6 && va == MAP_FAILED; i++) {
				snprintf(path, sizeof(path), "/sys/bus/pci/devices/%s/resource%d", de->d_name, i);
				va = map_bar(path, &start);
			}
		}
		if (d) closedir(d);
	}
	if (va == MAP_FAILED) printf("skipped: a device mapping: no PCI memory BAR here that can be mapped (name one as the argument)\n");
	else { printf("device mapping: %s\n", path); check("a device mapping", va, start); }
	return bad;
}

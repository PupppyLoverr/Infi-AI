# SPDX-License-Identifier: GPL-2.0-only
# The Linux end (xnudrmd); the macOS ends are kexts/XnuDrm and darwin/ (libxnudrm, built with Mesa).
# With libvirglrenderer installed (pkg-config), xnudrmd also serves a render node that is not virtio-gpu.
CFLAGS ?= -O2 -Wall -Wextra -Wno-unused-parameter
VIRGL ?= $(shell pkg-config --exists virglrenderer && echo 1)
SRC = xnudrmd.c
ifeq ($(VIRGL),1)
SRC += xnudrmd_virgl.c
# (override: these are needed to compile xnudrmd_virgl.c at all, also when CFLAGS comes from the command line)
override CFLAGS += -DXNUDRM_VIRGL $(shell pkg-config --cflags virglrenderer)
LDLIBS += $(shell pkg-config --libs virglrenderer)
endif
xnudrmd: $(SRC) xnudrm_proto.h xnudrmd.h .virgl
	$(CC) $(CFLAGS) -o $@ $(SRC) $(LDLIBS)
# with or without the backend is part of what xnudrmd is made from: virglrenderer installed after a first build (as
# tools/dgx-spark's README has it) builds xnudrmd again, where make saw nothing newer than it
.virgl: FORCE
	@echo '$(VIRGL)' | cmp -s - $@ || echo '$(VIRGL)' > $@
FORCE:
# xnudrmd with the backend, as a Venus client meets it, on the host's Vulkan drivers (no fki, no hosted system)
ifeq ($(VIRGL),1)
test-virgl: test/virgl.c $(SRC) xnudrm_proto.h xnudrmd.h
	$(CC) $(CFLAGS) -I. -o test/virgl test/virgl.c xnudrmd_virgl.c $(LDLIBS)
	test/virgl
else
test-virgl:
	@echo "xnudrmd's virglrenderer backend: NOT TESTED: no virglrenderer to build it with (pkg-config virglrenderer)"
endif
clean:
	rm -f xnudrmd test/virgl .virgl

#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/vm-xnu-rebuild.sh on a new host: nothing has made /lib/firmware/xnu yet, and there is no Apple kernelcache
# (out/kc/kc27.bin), which is all the PureDarwin collection (scripts/free-kc/build.sh) is documented to need. The script
# runs as it is on stand-ins: ~/w/dvm is a made-up macOS-DroidVM tree (a git tree for XNU, a build-kernel27.sh that
# writes a "kernel", a build-native-kext.py that writes a kext's objects.json), xnu-patches holds one small patch, and
# sudo runs its command with /lib/firmware inside the sandbox.
. "$(dirname "$0")/lib.sh"
sandbox_repo kexts
P=$T/repo; D=$HOME/w/dvm; X=$D/out/k27/xnu
rm -rf "$P/scripts/kclink"; mkdir -p "$P/scripts/kclink"	# the linker and the AMFI patch: stand-ins that write the collection
cat > "$P/scripts/kclink/kclink.py" <<'EOF'
import sys
open(sys.argv[3], 'w').write('collection: ' + ' '.join(sys.argv[sys.argv.index('--native') + 1:sys.argv.index('--rebind')]) + '\n')
print('wrote', sys.argv[3])
EOF
echo 'pass' > "$P/scripts/kc-amfi-platform.py"
# the kernel's slots from layout numbers (scripts/free-kc/kernel-slots.py, which the tree may or may not have yet): a stand-in
SLOTS=$P/scripts/free-kc/kernel-slots.py; mkdir -p "$P/scripts/free-kc"
cat > "$SLOTS" <<'EOF'
import sys
assert sys.argv[1].endswith('scripts/free-kc/layout.json') and sys.argv[2] == 'out/k27/kernel-k27.macho', sys.argv
open(sys.argv[sys.argv.index('--write') + 1], 'w').write('slots from the layout numbers\n'); print('kernel slots: written')
EOF
mkdir -p "$P/xnu-patches/include" "$P/xnu-patches/corecrypto/include/corecrypto" "$P/xnu-patches/corecrypto/kernel" \
  "$P/build/IONetworkingFamily-188" "$X/bsd" "$D/k27" "$D/scripts" "$D/out/kc" "$VM/lib/firmware"
: > "$P/build/IONetworkingFamily-188/IOEthernetController.h"	# as fetched already: the test clones nothing
echo '#define LINUX64 1' > "$P/xnu-patches/include/linux64.h"
echo '/* cc */' > "$P/xnu-patches/corecrypto/include/corecrypto/cc.h"; echo '/* glue */' > "$P/xnu-patches/corecrypto/kernel/glue.c"
printf 'int a;\n' > "$X/bsd/a.c"
git -C "$X" init -q && git -C "$X" add . && git -C "$X" -c user.name=t -c user.email=t@t commit -qm xnu
cat > "$P/xnu-patches/0001-a.patch" <<'EOF'
--- a/bsd/a.c
+++ b/bsd/a.c
@@ -1 +1 @@
-int a;
+int a = 1;
EOF
echo 'mkdir -p out/k27; echo kernel > out/k27/kernel-k27.macho; echo built >> k27/builds; echo "kernel built"' > "$D/k27/build-kernel27.sh"
: > "$D/k27/segaddrs.sh"; echo 'pass' > "$D/k27/kc27-layout.py"
cat > "$D/scripts/build-native-kext.py" <<'EOF'
import os, sys
d = 'out/x12377/native-kexts/' + sys.argv[1]; os.makedirs(d, exist_ok=True)
open(d + '/objects.json', 'w').write('[]\n'); print('kext', sys.argv[1])
EOF
stub sudo <<'EOF'
a=(); for x in "$@"; do case $x in /lib/firmware/*) x=$VM$x;; esac; a[${#a[@]}]=$x; done
exec "${a[@]}"
EOF
kexts=$(cd "$P/kexts" && ls */*.json | sed 's|/.*||' | sort | tr '\n' ' ')
built() { (cd "$D/out/x12377/native-kexts" 2>/dev/null && ls */objects.json | sed 's|/.*||' | sort | tr '\n' ' '); }
F=$VM/lib/firmware/xnu

/bin/bash "$P/scripts/vm-xnu-rebuild.sh" > "$T/out" 2>&1; s=$?
check 'a host without /lib/firmware/xnu: the script ends well' [ $s = 0 ]
check '... with the kernel installed as /lib/firmware/xnu/kernel.macho' cmp -s "$D/out/k27/kernel-k27.macho" "$F/kernel.macho"
check "without Apple's kernelcache: xnulinux's kexts are built all the same ($kexts)" [ "$(built)" = "$kexts" ]
check '... against build/netinc, which is set up' [ -L "$P/build/netinc/IOKit/network" ]
check '... and no collection of Apple kexts is linked or installed' not test -e "$F/kc.bin" -o -e "$D/out/k27/natives.txt"
check '... the kernel gets its slots from the layout numbers, there being no kernelcache to take them from' \
  grep -qx 'slots from the layout numbers' "$D/k27/segaddrs.sh"
check '... and is linked again for them, once' [ "$(grep -c built "$D/k27/builds")" = 2 ]
[ $FAILED = 0 ] || sed 's/^/    | /' "$T/out"

# a tree that has no kernel-slots.py: the slots are left as they are and the script ends as well
rm -rf "$D/out/x12377" "$VM/lib/firmware/xnu" "$SLOTS" "$D/k27/builds"; : > "$D/k27/segaddrs.sh"
/bin/bash "$P/scripts/vm-xnu-rebuild.sh" > "$T/out" 2>&1; s=$?
check 'without kernel-slots.py in the tree: the script ends well, the slots untouched, the kernel linked once' \
  [ $s = 0 -a ! -s "$D/k27/segaddrs.sh" -a "$(grep -c built "$D/k27/builds")" = 1 ]
[ $FAILED = 0 ] || sed 's/^/    | /' "$T/out"

rm -rf "$D/out/x12377" "$VM/lib/firmware/xnu"; echo kernelcache > "$D/out/kc/kc27.bin"
/bin/bash "$P/scripts/vm-xnu-rebuild.sh" > "$T/out" 2>&1; s=$?
check "with the kernelcache: the script ends well, the kexts are built" [ $s = 0 -a "$(built)" = "$kexts" ]
check '... natives.txt lists them for scripts/apple-kc/package.sh' [ "$(sed 's|.*/||; s|\.json$||' "$D/out/k27/natives.txt" 2>/dev/null | sort | tr '\n' ' ')" = "$kexts" ]
check '... and the collection linked with them is installed as kc.bin' grep -qs "^collection: .*XnuDisk.json" "$F/kc.bin"
[ $FAILED = 0 ] || sed 's/^/    | /' "$T/out"
finish

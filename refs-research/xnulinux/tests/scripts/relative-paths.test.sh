#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Scripts that are given a path relative to where the caller stands, and work somewhere else: console-send-file.sh
# (LOCAL), xnulinux-boot.sh (MAC_CONF, DISK), free-kc/build.sh (FREE_KC_WORK, DVM) and build-host.sh (W). Each runs as
# it is from $T/there, a directory that is not the repository's, on stand-ins for what it calls.
. "$(dirname "$0")/lib.sh"
sandbox_repo kexts
R=$T/repo; mkdir -p "$T/there"; cd "$T/there" || exit 1

# console-send-file.sh: the hosted console gets the caller's file, whatever the repository has under that name
echo 127.0.0.1 > "$T/vm.ip"; : > "$T/vmkey"; stub_ssh	# scripts/vm.sh's, next to the repository; the VM's console-in
echo "the caller's notes" > notes.txt; echo "the repository's notes" > "$R/notes.txt"; echo 'only here' > here.txt
typed() { sed -n 's/^echo \(.*\) >> .*/\1/p' "$VM/var/tmp/console-in" | python3 -c 'import base64, sys; sys.stdout.write(base64.b64decode(sys.stdin.read()).decode())'; }
/bin/bash "$R/scripts/console-send-file.sh" here.txt /tmp/h > "$T/out" 2>&1; s=$?
check 'console-send-file.sh FILE (a file where the caller is): it is what gets typed' [ "$s $(typed)" = '0 only here' ]
: > "$VM/var/tmp/console-in"; /bin/bash "$R/scripts/console-send-file.sh" notes.txt /tmp/n >> "$T/out" 2>&1
check "... also when the repository has a file of that name" [ "$(typed)" = "the caller's notes" ]
: > "$VM/var/tmp/console-in"; /bin/bash "$R/scripts/console-send-file.sh" nothing.txt /tmp/precious >> "$T/out" 2>&1; s=$?
check '... and a file that is not there is an error, with nothing typed (the first line typed removes REMOTE)' [ $s != 0 -a ! -s "$VM/var/tmp/console-in" ]
[ $FAILED = 0 ] || sed 's/^/    | /' "$T/out"

# xnulinux-boot.sh: stopped at its first build step by a make that fails, after it has read its configuration and before
# it does anything as root (sudo fails too). The stand-in make writes down the DISK the script would boot from (the
# caller exported it, so make inherits it).
f=$FAILED
stub sudo <<'EOF'
exit 1
EOF
stub make <<'EOF'
echo "$PWD" > "$T/make.cwd"; echo "${DISK:-}" > "$T/make.disk"; echo "make (stand-in): stops the boot here" >&2; exit 2
EOF
printf 'ROOT_UUID=1111\nBOOT_UUID=2222\n' > pd.conf; : > pd.img
if [ -e /etc/xnulinux.conf ] || [ -d /sys/module/xnu ]; then
  echo 'skip  xnulinux-boot.sh: this host has a hosted system of its own (/etc/xnulinux.conf would be read)'
else
  MAC_CONF=pd.conf DISK=pd.img /bin/bash "$R/scripts/xnulinux-boot.sh" > "$T/out" 2>&1
  check 'xnulinux-boot.sh with MAC_CONF=pd.conf (docs/free-kc.md): the UUIDs are read from the file where the caller is' called '^make '
  check '... and DISK=pd.img is that directory'"'"'s pd.img, from where xnu.ko is loaded' \
    eval '[ -s "$T/make.cwd" ] && cd "$(cat "$T/make.cwd")" && [ "$(cat "$T/make.disk")" -ef "$T/there/pd.img" ]'
  cd "$T/there" || exit 1
  [ $FAILED = $f ] || sed 's/^/    | /' "$T/out"
fi

# free-kc/build.sh: kcc.py and genspec.py are the repository's; the compiler (the k27 recipes' "cc"), mk-template.py and
# kclink.py are stand-ins that write their output, PureDarwin and Mbed TLS are a few empty files "fetched already"
f=$FAILED
stub sudo <<'EOF'
exec "$@"
EOF
stub cc <<'EOF'
while [ $# -gt 1 ]; do [ "$1" != -o ] || : > "$2"; shift; done
EOF
stub git <<'EOF'
[ "$1 $3 $4" != '-C rev-parse HEAD' ] || cat "$2/.rev"
EOF
echo 'import sys; open(sys.argv[4], "w").write("template\n")' > "$R/scripts/free-kc/mk-template.py"
rm -rf "$R/scripts/kclink"; mkdir -p "$R/scripts/kclink"
cat > "$R/scripts/kclink/kclink.py" <<'EOF'
import json, sys
natives = sys.argv[sys.argv.index('--native') + 1:]
for n in natives: json.load(open(json.load(open(n))['objects']))	# as kclink reads a kext's description and its object list
open(sys.argv[3], 'w').write('collection\n'); print('wrote', sys.argv[3], 'with', len(natives), 'kexts')
EOF
E=free-kc/src/pd/src/Kernel/Extensions; O=dvm/out/x12377; X=dvm/out/k27
mkdir -p $E/amfi $E/pthread $E/IOStorageFamily $E/HFSEncodings $E/hfs/core free-kc/src/mbedtls $O/native-kexts \
  $O/build/xnu/k27-obj/DEVELOPMENT_ARM64_BCM2837/iokit/DEVELOPMENT $O/build/xnu/k27-obj/DEVELOPMENT_ARM64_BCM2837/bsd/DEVELOPMENT \
  $O/build/xnu/k27h-dst/System/Library/Frameworks/Kernel.framework/Versions/A/PrivateHeaders $X/xnu "$T/out.d"
sed -n 's/.* PD_REV=\([0-9a-f]*\).*/\1/p' "$R/scripts/free-kc/build.sh" > free-kc/src/pd/.rev
sed -n 's/^MBEDTLS_REV=//p' "$R/kexts/free/corecrypto/mbedtls.rev" > free-kc/src/mbedtls/.rev
for k in amfi/Info pthread/pthread-Info IOStorageFamily/Info HFSEncodings/Info hfs/Info; do
  printf '<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>org.test.%s</string></dict></plist>\n' "${k%%/*}" > "$E/$k.plist"
done
: > $E/IOStorageFamily/a.cpp; : > $E/HFSEncodings/a.c; : > $E/HFSEncodings/b.cpp; : > $E/hfs/core/a.c; : > $E/hfs/core/b.cpp; : > $E/hfs/core/kext-config.h
for r in iokit/DEVELOPMENT/IOPlatformExpert.cpo.json bsd/DEVELOPMENT/bsd_init.o.json; do
  echo '{"directory": "/", "file": "x.c", "arguments": ["cc", "-c", "x.c", "-o", "x.o"]}' > "$O/build/xnu/k27-obj/DEVELOPMENT_ARM64_BCM2837/$r"
done
for k in XnuCPU XnuDisk XnuDrm; do mkdir -p $O/native-kexts/$k; echo '[]' > $O/native-kexts/$k/objects.json; done	# vm-xnu-rebuild.sh's
echo 'Darwin Kernel Version 27.2.0: a stand-in' > $X/kernel-k27.macho
DVM=dvm FREE_KC_WORK=free-kc OUT=$T/out.d/kc-free.bin /bin/bash "$R/scripts/free-kc/build.sh" > "$T/out" 2>&1; s=$?
check 'free-kc/build.sh with DVM=dvm FREE_KC_WORK=free-kc: the collection is built' [ $s = 0 -a -s "$T/out.d/kc-free.bin" ]
check '... its objects in FREE_KC_WORK, none beside the sources they were compiled from' \
  eval '[ -s free-kc/obj/corecrypto/objects.json ] && [ -z "$(find "$R/kexts" free-kc/src -name objects.json)" ]'
[ $FAILED = $f ] || tail -8 "$T/out" | sed 's/^/    | /'

# build-host.sh: make gets the output directory the header promises ($W/out/linux), whatever directory it runs in
f=$FAILED
mkdir -p w/src/linux/drivers w/src/linux/scripts
echo 'source "drivers/fki/Kconfig"' > w/src/linux/drivers/Kconfig; echo 'obj-$(CONFIG_FKI) += fki/' > w/src/linux/drivers/Makefile
printf '#!/bin/sh\n' > w/src/linux/scripts/config; chmod +x w/src/linux/scripts/config
for c in rsync nproc; do stub $c <<'EOF'
echo 4
EOF
done
stub make <<'EOF'
for a in "$@"; do case $a in O=*) o=${a#O=};; Image) mkdir -p "$o/arch/arm64/boot" && : > "$o/arch/arm64/boot/Image";; esac; done
EOF
W=w /bin/bash "$R/scripts/build-host.sh" > "$T/out" 2>&1; s=$?
check 'build-host.sh with W=w: the kernel is built into w/out/linux, not into the Linux tree' \
  [ $s = 0 -a -e w/out/linux/arch/arm64/boot/Image -a ! -e w/src/linux/w ]
[ $FAILED = $f ] || sed 's/^/    | /' "$T/out"
finish

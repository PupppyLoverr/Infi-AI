#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/apple-kc/package.sh and link.sh on stand-ins for what they read (macOS-DroidVM's tree, a kit, a kernelcache),
# with the linker replaced by one that only writes its output: what the two scripts do around the link. Run it on
# Linux, where package.sh is run (bash 4, sha256sum): tests/apple-kc/run.sh
set -u
command -v sha256sum >/dev/null && [ "${BASH_VERSINFO[0]}" -ge 4 ] || { echo "skipped: needs bash 4 and sha256sum (Linux)"; exit 0; }
POC=$(cd "$(dirname "$0")/../.." && pwd); T=$(mktemp -d); trap 'rm -rf "$T"' EXIT; fails=0
check() { local name=$1; shift; if "$@"; then echo "ok   $name"; else echo "FAIL $name"; sed 's/^/       /' "$T/out"; fails=$((fails + 1)); fi; }

dvm() {	# what package.sh reads of macOS-DroidVM's tree, in $T/w/dvm
	rm -rf "$T/w"; mkdir -p "$T/w/dvm/out/k27" "$T/w/dvm/out/kc" "$T/w/dvm/configs" "$T/w/dvm/obj" "$T/w/notes"
	echo kernel > "$T/w/dvm/out/k27/kernel-k27.macho"; echo kc > "$T/w/dvm/out/kc/kc27.bin"; echo obj > "$T/w/dvm/obj/K.o"
	echo '{"ipsw":{"url":"https://example.invalid/x.ipsw"}}' > "$T/w/dvm/configs/macos-27.2-26B123.json"
	echo "[\"$T/w/dvm/obj/K.o\"]" > "$T/w/dvm/obj/objects.json"; echo notes > "$T/w/notes/todo.txt"
	echo '{"name":"K","objects":"obj/objects.json"}' > "$T/w/dvm/K.json"; echo "$T/w/dvm/K.json" > "$T/w/dvm/out/k27/natives.txt"
}
package() { DVM=$T/w/dvm bash "$POC/scripts/apple-kc/package.sh" "$@" > "$T/out" 2>&1; }
sha_in() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["kernelcache_sha256"])' "$1/kit.json"; }

dvm
check "package.sh: a kit where there was nothing" package "$T/kit"
check "package.sh: kit.json names the kernelcache's sha256" [ "$(sha_in "$T/kit")" = "$(sha256sum "$T/w/dvm/out/kc/kc27.bin" | cut -d' ' -f1)" ]
touch "$T/kit/stale"
check "package.sh: a kit is replaced whole" eval 'package "$T/kit" && [ ! -e "$T/kit/stale" ] && [ -s "$T/kit/kernel.macho" ]'
mkdir "$T/empty"
check "package.sh: an empty directory becomes the kit" eval 'package "$T/empty" && [ -s "$T/empty/kit.json" ]'
check "package.sh: a directory that is not a kit is refused" eval '! package "$T/w"'
check "package.sh: ...and nothing in it is removed" eval '[ -s "$T/w/notes/todo.txt" ] && [ -s "$T/w/dvm/out/k27/kernel-k27.macho" ]'
dvm; echo precious > "$T/afile"
check "package.sh: a file is refused and kept" eval '! package "$T/afile" && [ "$(cat "$T/afile")" = precious ]'
dvm; rm "$T/w/dvm/out/kc/kc27.bin"
check "package.sh: no kernelcache, no kit" eval '! package "$T/kit2" && [ ! -e "$T/kit2" ]'
: > "$T/w/dvm/out/kc/kc27.bin"
check "package.sh: an empty kernelcache, no kit" eval '! package "$T/kit2" && [ ! -e "$T/kit2" ]'

# link.sh in a copy of the tree whose kclink.py writes its OUTPUT argument and whose kc-amfi-platform.py does nothing
mkdir -p "$T/poc/scripts/apple-kc" "$T/poc/scripts/kclink" "$T/lk/kit" "$T/bin"; cp "$POC/scripts/apple-kc/link.sh" "$T/poc/scripts/apple-kc/"
printf 'import sys\nopen(sys.argv[3], "w").write("NEW\\n"); print("wrote", sys.argv[3])\n' > "$T/poc/scripts/kclink/kclink.py"
: > "$T/poc/scripts/kc-amfi-platform.py"; : > "$T/lk/kit/kernel.macho"; printf '\xcf\xfa\xed\xfe a stand-in kernelcache\n' > "$T/lk/kc.macho"
printf '{"kernelcache_sha256":"%s","kernelcache":"a stand-in","ipsw":"https://example.invalid/x.ipsw","amfi_platform":26,"keep":["com.apple.a"],"rebind":["0x1=_x"],"native":["kexts/K/K.json"]}\n' \
	"$(sha256sum "$T/lk/kc.macho" | cut -d' ' -f1)" > "$T/lk/kit/kit.json"
link() { ( cd "$T/lk" && "$@" bash "$T/poc/scripts/apple-kc/link.sh" kit kc.macho out.bin ) > "$T/out" 2>&1; }
check "link.sh: links with its work directory in the kit" eval 'link env && [ -s "$T/lk/out.bin" ] && [ -s "$T/lk/kit/.work/kclink.log" ]'
rm -f "$T/lk/out.bin"
check "link.sh: a relative APPLE_KC_WORK" eval 'link env APPLE_KC_WORK=work && [ -s "$T/lk/out.bin" ] && [ -s "$T/lk/work/kclink.log" ]'

# the install into /lib/firmware/xnu, in a mount namespace of our own where that can be made (not everywhere: skipped then)
printf '#!/bin/sh\necho "sudo: a terminal is required to read the password" >&2; exit 1\n' > "$T/bin/sudo"; chmod +x "$T/bin/sudo"
inns() { unshare -rm sh -c 'mount -t tmpfs none /lib/firmware 2>/dev/null && mkdir /lib/firmware/xnu && echo OLD > /lib/firmware/xnu/kc-apple.bin && '"$1" 2>/dev/null; }
if inns true; then
	export T
	check "link.sh: a failed install is a failed link.sh" eval '! inns "cd $T/lk && PATH=$T/bin:\$PATH bash $T/poc/scripts/apple-kc/link.sh kit kc.macho > $T/out 2>&1"'
	check "link.sh: ...and it does not say 'boot it'" eval '! grep -q "boot it" "$T/out"'
else
	echo "skip link.sh: a failed install (no unprivileged mount namespace here to stand in for /lib/firmware/xnu)"
fi
[ $fails = 0 ] && echo "apple-kc tests: pass" || { echo "apple-kc tests: $fails failed"; exit 1; }

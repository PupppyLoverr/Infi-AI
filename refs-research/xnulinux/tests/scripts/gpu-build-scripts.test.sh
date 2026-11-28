#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# The GPU build scripts up to where they hand over to the real build: gpu/venus/build-linux.sh and build-darwin.sh,
# gpu/xnu-cpu-metal/scripts/fetch-sources.sh, tools/dgx-spark/gpu/build-virglrenderer.sh. The scripts run as they are;
# curl, tar, meson, ninja, git, uv and the rest are stand-ins. curl "downloads" a recognisable file, or stops half way
# as a transfer that is cut does; tar takes only what a whole download left; meson refuses a prefix that is not an
# absolute path, as the real one does.
. "$(dirname "$0")/lib.sh"
sandbox_repo gpu/venus gpu/xnu-cpu-metal/scripts tools/dgx-spark/gpu gpu/xnu-drm
P=$T/repo; export T
PY=$(command -v python3)
for c in apt-get ldconfig pip patch; do stub $c < /dev/null; done
stub vulkaninfo <<'EOF'
echo "deviceName = stand-in"
EOF
stub sudo <<'EOF'
while [ $# -gt 0 ]; do case $1 in *=*) export "$1"; shift;; *) break;; esac; done
exec "$@"
EOF
stub curl <<'EOF'
while [ $# -gt 1 ]; do case $1 in -*o) out=$2; shift;; esac; shift; done	# -fsSLo FILE URL
case $(cat "$T/curl") in
ok) echo "TARBALL of $1" > "$out";;
cut) echo "half of it" > "$out"; echo "curl: (18) transfer closed with outstanding read data remaining" >&2; exit 18;;
esac
EOF
stub tar <<'EOF'
while [ $# -gt 0 ]; do case $1 in -C) d=$2; shift;; -xf) f=$2; shift;; esac; shift; done
grep -q '^TARBALL' "$f" || { echo "tar: This does not look like a tar archive" >&2; exit 2; }
[ ! -e "$T/tar-fails" ] || { : > "$d/half-unpacked"; echo "tar: Unexpected EOF in archive" >&2; exit 2; }
: > "$d/meson.build"
EOF
stub meson <<'EOF'
for a in "$@"; do case $a in --prefix=*) p=${a#--prefix=}
  case $p in /*) echo "$p" > "$T/prefix";; *) echo "ERROR: prefix value '$p' must be an absolute path" >&2; exit 1;; esac;; esac; done
EOF
stub ninja <<'EOF'
p=$(cat "$T/prefix")
case " $* $p" in *" install $T"/*) mkdir -p "$p/share/vulkan/icd.d"; : > "$p/share/vulkan/icd.d/virtio_icd.aarch64.json";; esac	# (never outside the sandbox)
EOF
stub glslangValidator <<'EOF'
while [ $# -gt 1 ]; do case $1 in -o) : > "$2";; esac; shift; done
EOF
stub cc <<'EOF'
while [ $# -gt 1 ]; do case $1 in -o) printf '#!/bin/sh\necho "compute: ok"\n' > "$2"; chmod +x "$2";; esac; shift; done
EOF
stub pkg-config <<'EOF'
echo 1.2.0
EOF
stub git <<'EOF'
[ "$1" != -C ] || { [ "$3" != init ] || mkdir -p "$2/.git"; }
EOF
export PY
cat > "$T/bin/python3" <<'EOF'	# -m venv DIR: a venv with the stand-ins in it; anything else is the real python3
#!/bin/bash
echo "python3 $*" >> "$T/calls"
if [ "$1 $2" = "-m venv" ]; then mkdir -p "$3/bin"; cp "$T/bin/meson" "$T/bin/pip" "$3/bin/"; exit 0; fi
exec "$PY" "$@"
EOF
chmod +x "$T/bin/python3"
tarballs() { ls "$1" 2>/dev/null | grep -c -E '\.tar\.(xz|gz)$'; }	# files there that pass for a whole tarball

# ---- gpu/venus/build-linux.sh
mkdir -p "$T/cwd"; echo ok > "$T/curl"
(cd "$T/cwd" && W=rel/w /bin/bash "$P/gpu/venus/build-linux.sh" > "$T/out" 2>&1); s=$?
check 'build-linux.sh with a W relative to where it is run from: it builds' [ $s = 0 ]
check "... meson's prefix is the absolute W/out" called "^meson setup .* --prefix=$T/cwd/rel/w/out "
check '... and the test it ends with ran' grep -q '^compute: ok' "$T/out"
echo cut > "$T/curl"; : > "$T/calls"
W=$T/w1 /bin/bash "$P/gpu/venus/build-linux.sh" > "$T/out" 2>&1; s=$?
check 'build-linux.sh, the download cut short: it stops' [ $s != 0 ]
check '... and leaves nothing that the next run would take for the tarball' [ "$(tarballs "$T/w1")" = 0 ]
echo ok > "$T/curl"
W=$T/w1 /bin/bash "$P/gpu/venus/build-linux.sh" > "$T/out" 2>&1; s=$?
check '... so the next run downloads again and builds' [ $s = 0 -a "$(grep -c '^curl ' "$T/calls")" = 2 ]

# ---- tools/dgx-spark/gpu/build-virglrenderer.sh
echo cut > "$T/curl"; : > "$T/calls"
W=$T/w2 /bin/bash "$P/tools/dgx-spark/gpu/build-virglrenderer.sh" > "$T/out" 2>&1; s=$?
check 'build-virglrenderer.sh, the download cut short: it stops, nothing kept as the tarball' [ $s != 0 -a "$(tarballs "$T/w2")" = 0 ]
echo ok > "$T/curl"
W=$T/w2 /bin/bash "$P/tools/dgx-spark/gpu/build-virglrenderer.sh" > "$T/out" 2>&1; s=$?
check '... and the next run downloads again and builds' [ $s = 0 -a "$(grep -c '^curl ' "$T/calls")" = 2 ]

# ---- gpu/xnu-cpu-metal/scripts/fetch-sources.sh
M=$P/gpu/xnu-cpu-metal; mkdir -p "$M/src"
echo cut > "$T/curl"
/bin/bash "$M/scripts/fetch-sources.sh" > "$T/out" 2>&1; s=$?
check 'fetch-sources.sh, the download cut short: it stops, nothing kept as the tarball' [ $s != 0 -a "$(tarballs "$M/build")" = 0 ]
echo ok > "$T/curl"; : > "$T/tar-fails"
/bin/bash "$M/scripts/fetch-sources.sh" > "$T/out" 2>&1; s=$?
check 'fetch-sources.sh, the unpacking failing half way: it stops, and src/mesa is not there for the next run to take' [ $s != 0 -a ! -e "$M/src/mesa" ]
rm "$T/tar-fails"
/bin/bash "$M/scripts/fetch-sources.sh" > "$T/out" 2>&1; s=$?
check '... the next run unpacks it whole' [ $s = 0 -a -e "$M/src/mesa/meson.build" -a ! -e "$M/src/mesa/half-unpacked" ]

# ---- gpu/venus/build-darwin.sh: uv where its installer puts it (~/.local/bin), which the script's own PATH leaves out
if [ -x /opt/homebrew/bin/python3 ]; then
  echo "skipped: build-darwin.sh and uv (this machine has Homebrew's python3, which the script would use without the fix)"
else
  mkdir -p "$HOME/.local/bin"
  printf '#!/bin/sh\necho "reached the download" >> "$T/calls"; exit 7\n' > "$T/curl-stop"; chmod +x "$T/curl-stop"
  cat > "$HOME/.local/bin/uv" <<'EOF'	# uv venv ... DIR: a venv with the stand-ins, whose curl ends the script at the download
#!/bin/bash
echo "uv $*" >> "$T/calls"
if [ "$1" = venv ]; then for d; do :; done; mkdir -p "$d/bin"; cp "$T/bin/meson" "$d/bin/"; ln -s "$PY" "$d/bin/python3"; cp "$T/curl-stop" "$d/bin/curl"; fi
EOF
  chmod +x "$HOME/.local/bin/uv"; : > "$T/calls"
  PATH=$HOME/.local/bin:$PATH W=$T/w3 /bin/bash "$P/gpu/venus/build-darwin.sh" > "$T/out" 2>&1; s=$?
  check "build-darwin.sh makes its Python environment with the user's uv" called '^uv venv .* venv$'
  check '... installs into it with that uv, and goes on to the download' [ "$(grep -c '^uv pip install' "$T/calls")$(grep -c '^reached the download' "$T/calls")" = 11 ]
fi
finish

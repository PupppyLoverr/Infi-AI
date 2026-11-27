#!/bin/bash
# Publish a folder of CI screenshots to docs/shots/<name> on this branch.
# Conflict-proof: each attempt starts from the branch tip and only replaces
# our own folder, so concurrent publishers (VM, ISO) never collide.
# Usage: publish-shots.sh <staged-dir> <docs/shots/name> <message>
set -u
src="$1"; dest="$2"; msg="$3"
[ -d "$src" ] && [ -n "$(ls -A "$src")" ] || exit 0
stage=$(mktemp -d); cp -r "$src"/. "$stage"/
git config user.name "solyra-ci"; git config user.email "solyra-ci@users.noreply.github.com"
for i in 1 2 3 4 5; do
  git fetch -q origin "$GITHUB_REF_NAME" && git reset -q --hard FETCH_HEAD
  rm -rf "$dest"; mkdir -p "$dest"; cp -r "$stage"/. "$dest"/
  git add -A "$dest"
  git commit -qm "$msg [skip ci]" || exit 0
  git push -q origin "HEAD:$GITHUB_REF_NAME" && exit 0
  sleep $((i * 4))
done
exit 1

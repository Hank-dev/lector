#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT

# Spaces catch broken Makefile quoting. DESTDIR must not touch host state.
prefix='/usr/local/lector test'
make install-files DESTDIR="$stage" PREFIX="$prefix"
test -x "$stage$prefix/bin/lector"
desktop-file-validate "$stage$prefix/share/applications/org.omarchy.Lector.desktop"
cmp LICENSE "$stage$prefix/share/licenses/lector/LICENSE"
test -f "$stage$prefix/share/icons/hicolor/scalable/apps/org.omarchy.Lector.svg"
if make default DESTDIR="$stage" PREFIX="$prefix"; then
  echo 'make default must refuse a staging directory' >&2
  exit 1
fi
make uninstall DESTDIR="$stage" PREFIX="$prefix"
test -z "$(find "$stage" -type f -print -quit)"
echo 'Staged installation and removal passed'

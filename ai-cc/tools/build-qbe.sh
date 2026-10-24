#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out=${1:-"$root/tools/qbe"}
tmp=${TMPDIR:-/tmp}/ai-cc-qbe
rm -rf "$tmp"
git clone --depth 1 git://c9x.me/qbe.git "$tmp"
make -C "$tmp"
mkdir -p "$(dirname "$out")"
cp "$tmp/qbe" "$out"
rm -rf "$tmp"
printf '%s\n' "$out"

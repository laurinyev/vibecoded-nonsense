#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
prefix=${1:-/usr/local}
cargo build --release --manifest-path "$root/Cargo.toml"
mkdir -p "$prefix/bin" "$prefix/lib/include"
install -m 755 "$root/target/release/ai-cc" "$prefix/bin/ai-cc"
install -m 644 "$root"/lib/include/*.h "$prefix/lib/include/"
mkdir -p "$prefix/libexec/ai-cc/crt"
install -m 644 "$root/target/release/build/ai-cc-"*/out/crtbegin.o "$prefix/libexec/ai-cc/crt/crtbegin.o"
install -m 644 "$root/target/release/build/ai-cc-"*/out/crtend.o "$prefix/libexec/ai-cc/crt/crtend.o"
install -m 644 "$root/target/release/build/ai-cc-"*/out/crtstart.o "$prefix/libexec/ai-cc/crt/crtstart.o"
if [ -x "$root/tools/qbe" ]; then
    mkdir -p "$prefix/libexec/ai-cc"
    install -m 755 "$root/tools/qbe" "$prefix/libexec/ai-cc/qbe"
fi
printf '%s\n' "$prefix/bin/ai-cc"

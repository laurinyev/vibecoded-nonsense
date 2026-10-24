#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo build --manifest-path "$root/ai-cc/Cargo.toml" --release
cargo build --manifest-path "$root/ai-make/Cargo.toml"
"$root/ai-make/target/debug/ai-make" "$root/grid-runner/AiMake.yaml"
exec "$root/grid-runner/build/grid-runner"

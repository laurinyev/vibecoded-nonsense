#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bin="$root/target/debug/ai-cc"
qbe=${QBE:-$root/tools/qbe}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
cargo build --quiet --manifest-path "$root/Cargo.toml"
pass=0
fail=0
check(){
    name=$1
    shift
    if "$@"; then
        printf 'PASS %s\n' "$name"
        pass=$((pass+1))
    else
        printf 'FAIL %s\n' "$name"
        fail=$((fail+1))
    fi
}
check_preprocessed(){
    "$bin" -E "$root/tests/cases/header_stdbool.c" >"$work/preprocessed.c"
    grep -q '_Bool enabled' "$work/preprocessed.c"
}
check_qbe_stdout(){
    "$bin" -S "$root/tests/cases/function_call.c" >"$work/stdout.ssa"
    grep -q 'export function w \$main' "$work/stdout.ssa"
}
check_qbe_file(){
    "$bin" -S "$root/tests/cases/function_call.c" -o "$work/output.ssa"
    test -s "$work/output.ssa"
}
check_object(){
    AI_CC_QBE="$qbe" "$bin" -c "$root/tests/cases/function_call.c" -o "$work/output.o"
    file "$work/output.o" | grep -q 'relocatable'
}
check_linked(){
    AI_CC_QBE="$qbe" "$bin" "$root/tests/cases/glibc_output.c" -o "$work/program"
    bwrap --die-with-parent --unshare-user --unshare-pid --unshare-ipc --unshare-uts --unshare-cgroup --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --bind "$work" "$work" --chdir "$work" "$work/program" >"$work/output"
    printf 'hello from glibc\nvalue=42\n' | cmp -s - "$work/output"
}
check_freestanding(){
    AI_CC_QBE="$qbe" "$bin" --freestanding "$root/tests/cases/freestanding.c" -o "$work/freestanding"
    set +e
    bwrap --die-with-parent --unshare-user --unshare-pid --unshare-ipc --unshare-uts --unshare-cgroup --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --bind "$work" "$work" --chdir "$work" "$work/freestanding" >/dev/null 2>"$work/freestanding.err"
    status=$?
    set -e
    test "$status" -eq 42
}
check_help(){
    "$bin" --help >"$work/help"
    grep -q 'Preprocess source only' "$work/help" && grep -q 'Compile to object file without linking' "$work/help"
}
check_conflict(){
    ! "$bin" -E -S "$root/tests/cases/function_call.c" >"$work/conflict" 2>&1
}
check "-E" check_preprocessed
check "-S stdout" check_qbe_stdout
check "-S -o" check_qbe_file
check "-c object" check_object
check "default link" check_linked
check "freestanding link" check_freestanding
check "--help descriptions" check_help
check "conflicting stages" check_conflict
printf 'RESULT %s/8 passed\n' "$pass"
test "$fail" -eq 0

# Tests

`compiler.rs` contains focused frontend/diagnostic regressions and one glibc
interop test. Set `QBE` to use an existing QBE binary. Repetitive CLI and
header smoke cases were removed; the structured conformance harness below is
the source of truth for language coverage.

`conformance.rs` is the structured C99 harness. Its fixtures are separated into
`conformance/valid`, `conformance/invalid`, `conformance/diagnostics`,
`conformance/preprocess`, and `conformance/runtime`. Invalid fixtures must be
rejected; diagnostic fixtures carry an `EXPECT-DIAGNOSTIC` marker; the
preprocessor fixture is compared with the host `cc`; and the runtime fixture is
compiled, linked, sandboxed, and checked by exit status.

Fixtures derive from constructs illustrated by `../c99.pdf`, with the current
coverage boundary recorded in `../C99_ROADMAP.md`.

Run:

```sh
cargo test --test compiler
cargo test --test conformance
```

Enable the optional differential test against the host C compiler with:

```sh
AI_CC_RUN_DIFFERENTIAL=1 cargo test --test conformance differential_integer_program_matches_host_compiler
```

Recommended warning and sanitizer checks are:

```sh
RUSTFLAGS='-D warnings' cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
RUSTFLAGS='-Zsanitizer=address' cargo +nightly test
```

The last command requires a nightly Rust toolchain and a sanitizer-capable
linker; it is an additional CI/local check rather than a prerequisite for the
default test suite.

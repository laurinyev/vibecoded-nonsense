# Code Markings

14 active markings.

## TODO

- `src/ast.rs:99` — switch/case/default, labels, and goto are lexed but cannot be represented.
- `src/lex.rs:404` — Number stores i64 only; unsigned constants above i64::MAX are rejected.
- `src/lex.rs:414` — wide and UTF-8 literal prefixes (L, u, U, u8) are lexed as identifiers.

## BUG

- `src/codegen.rs:243` — non-power-of-two sizes fall back to 8-byte stores; LoadZero has the same issue.
- `src/layout.rs:44` — 16-byte long double is lowered and accessed as 8-byte QBE d.
- `src/lower.rs:1987` — undeclared callee is assumed int(...) instead of being diagnosed.
- `src/lower.rs:2336` — initializer errors are ignored, leaving the static object silently zeroed.
- `src/pp.rs:811` — wrong macro argument count silently leaves the invocation unexpanded.
- `src/pp.rs:868` — malformed replacement tokenization is silently discarded.
- `src/sema.rs:576` — integer literal suffixes are ignored; only the value range selects int versus long.
- `src/sema.rs:581` — string literals have array type in C99, not char pointer.
- `src/sema.rs:1218` — undesignated union initializer blindly selects field 0 and ignores extras.

## HACK

None.

## HACK HACK

- `src/lib.rs:47` — hyphen heuristic for multiarch includes can select the wrong architecture.
- `src/sema.rs:915` — undeclared callees are silently accepted as an implicit-int extension.

## HACK HACK HACK

None.

# Marking Legend

- `TODO` — pending work.
- `BUG` — known correctness defect.
- `HACK` — low-severity workaround.
- `HACK HACK` — medium-severity workaround.
- `HACK HACK HACK` — high-severity workaround.

# Roadmap

## Conformance

See [`C99_ROADMAP.md`](C99_ROADMAP.md).

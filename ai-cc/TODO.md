# Code Markings

19 active markings.

## TODO

- `src/ast.rs:99` — switch/case/default, labels, and goto are lexed but cannot be represented.
- `src/lex.rs:404` — Number stores i64 only; unsigned constants above i64::MAX are rejected.
- `src/lex.rs:414` — wide and UTF-8 literal prefixes (L, u, U, u8) are lexed as identifiers.

## BUG

- `src/codegen.rs:243` — non-power-of-two sizes fall back to 8-byte stores; LoadZero has the same issue.
- `src/layout.rs:44` — 16-byte long double is lowered and accessed as 8-byte QBE d.
- `src/lower.rs:1656` — undeclared callee is assumed int(...) instead of being diagnosed.
- `src/lower.rs:2002` — initializer errors are ignored, leaving the static object silently zeroed.
- `src/parser.rs:281` — enumerator names are not bound in ordinary identifier scope.
- `src/parser.rs:1821` — cloning the LHS duplicates side effects in compound assignments.
- `src/parser.rs:2212` — postfix increment is lowered as prefix and only works on bare identifiers.
- `src/pp.rs:811` — wrong macro argument count silently leaves the invocation unexpanded.
- `src/pp.rs:868` — malformed replacement tokenization is silently discarded.
- `src/sema.rs:423` — integer literal suffixes are ignored; only the value range selects int versus long.
- `src/sema.rs:428` — string literals have array type in C99, not char pointer.
- `src/sema.rs:936` — undesignated union initializer blindly selects field 0 and ignores extras.
- `src/sema.rs:1261` — any address-of variable is treated as constant, including automatic objects.
- `src/sema.rs:1705` — stripping qualifiers here makes const/volatile pointers falsely compatible.

## HACK

None.

## HACK HACK

- `src/lib.rs:46` — hyphen heuristic for multiarch includes can select the wrong architecture.
- `src/sema.rs:689` — undeclared callees are silently accepted as an implicit-int extension.

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

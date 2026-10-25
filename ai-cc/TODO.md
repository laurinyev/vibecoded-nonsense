# Code Markings

54 active markings.

## TODO

- `src/ast.rs:99` — switch/case/default, labels, and goto are lexed but cannot be represented.
- `src/lex.rs:403` — Number stores i64 only; unsigned constants above i64::MAX are rejected.
- `src/lex.rs:413` — wide and UTF-8 literal prefixes (L, u, U, u8) are lexed as identifiers.
- `src/lex.rs:541` — C99 permits implementation-defined multi-character constants.
- `src/parser.rs:258` — enumerator values only accept a literal, not an integer constant expression.

## BUG

- `src/codegen.rs:23` — Long/LongLong fall through to w and are truncated to 32 bits.
- `src/codegen.rs:196` — signed char uses loadub; unsigned short falls through to signed loadw.
- `src/codegen.rs:230` — non-power-of-two sizes fall back to 8-byte stores; LoadZero has the same issue.
- `src/codegen.rs:250` — integer binary ops are hardcoded to w/signed; long, pointer, and unsigned math break.
- `src/codegen.rs:449` — string symbols already include '$', so this emits "$$strN".
- `src/diag.rs:59` — slicing at a byte offset panics when it splits a UTF-8 code point.
- `src/layout.rs:44` — 16-byte long double is lowered and accessed as 8-byte QBE d.
- `src/lex.rs:324` — hex integers ending in f/F (e.g. 0xFF) are misclassified as floats.
- `src/lex.rs:509` — \x, multi-digit octal, and \a/\b/\f/\v are missing; unknown escapes silently drop the backslash.
- `src/lower.rs:461` — integer usual arithmetic conversions ignore rank, signedness, and promotions.
- `src/lower.rs:1313` — loads the pointer type instead of the pointee type.
- `src/lower.rs:1438` — logical RHS result is copied raw instead of normalized to 0/1.
- `src/lower.rs:1550` — undeclared callee is assumed int(...) instead of being diagnosed.
- `src/lower.rs:1852` — initializer errors are ignored, leaving the static object silently zeroed.
- `src/lower.rs:1910` — block scopes do not save/restore vars and slots; shadowed locals alias.
- `src/main.rs:225` — first ld-linux match is accepted without checking ELF class or architecture.
- `src/parser.rs:166` — a missing struct/union tag silently becomes an empty tag.
- `src/parser.rs:269` — enumerator names are not bound in ordinary identifier scope.
- `src/parser.rs:1108` — this drops extern for comma-list objects; tentative_definition cannot see Extern.
- `src/parser.rs:1205` — scope is popped only after expect succeeds; a parse error leaks aliases.
- `src/parser.rs:1272` — for-init declarations use the enclosing scope and remain visible after the loop.
- `src/parser.rs:1315` — a bare ';' is not accepted as an empty statement and reaches expr().
- `src/parser.rs:1667` — negative or overflowing bounds silently become unspecified arrays.
- `src/parser.rs:1780` — cloning the LHS duplicates side effects in compound assignments.
- `src/parser.rs:1926` — cast type is ignored during integer constant evaluation.
- `src/parser.rs:2155` — postfix increment is lowered as prefix and only works on bare identifiers.
- `src/pp.rs:384` — __FILE__/__LINE__ expand specially but are absent here, so defined(__FILE__) is false.
- `src/pp.rs:779` — whitespace before '(' blocks invocation; C99 requires adjacency only in definitions.
- `src/pp.rs:790` — wrong macro argument count silently leaves the invocation unexpanded.
- `src/pp.rs:848` — malformed replacement tokenization is silently discarded.
- `src/pp.rs:1068` — rhs[0] panics when the right operand of ## is an empty argument.
- `src/pp.rs:1192` — &&/|| do not short-circuit; an unevaluated RHS can raise division-by-zero.
- `src/sema.rs:423` — all integer literals are typed as int; value and suffix are ignored.
- `src/sema.rs:427` — string literals have array type in C99, not char pointer.
- `src/sema.rs:935` — undesignated union initializer blindly selects field 0 and ignores extras.
- `src/sema.rs:1126` — a negative index is reported as non-constant and silently advances.
- `src/sema.rs:1219` — shift counts are unchecked and can panic or wrap.
- `src/sema.rs:1256` — any address-of variable is treated as constant, including automatic objects.
- `src/sema.rs:1308` — arrow member lvalue-ness incorrectly follows the base expression.
- `src/sema.rs:1693` — stripping qualifiers here makes const/volatile pointers falsely compatible.
- `src/sema.rs:1737` — integer usual arithmetic conversions ignore rank, signedness, and promotions.
- `src/symbol.rs:19` — Default leaves the stack empty; insert/lookup_current_scope then panic.

## HACK

None.

## HACK HACK

- `build.rs:8` — build-time assembler is hardcoded, breaking cross-compilation.
- `src/lib.rs:46` — hyphen heuristic for multiarch includes can select the wrong architecture.
- `src/parser.rs:2124` — the required last-named-parameter argument is optional and discarded.
- `src/pp.rs:525` — includes recurse without depth/cycle tracking; self-include can overflow the stack.
- `src/pp.rs:1037` — the current macro is already disabled during argument prescan, breaking f(f(2)).
- `src/pp.rs:1137` — spacing only separates identifier-like tokens; punctuators can fuse into comments/operators.
- `src/sema.rs:688` — undeclared callees are silently accepted as an implicit-int extension.

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

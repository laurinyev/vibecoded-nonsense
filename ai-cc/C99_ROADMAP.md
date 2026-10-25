# Roadmap to C99 Coverage

This roadmap is ordered by urgency. It treats hosted libc implementation as out of scope because glibc is provided externally, but it includes the seven freestanding headers required by C99 4 and the compiler support needed to consume them.

The target is a documented, test-backed C99 implementation for one declared target ABI. “Coverage” means accepting valid C99 programs, rejecting constraint violations with diagnostics, and producing behavior consistent with the selected implementation-defined choices. It does not require optional Annex F IEC 60559 conformance unless explicitly claimed.

Section headings carry status labels: `— done` means complete for the current target and supported subset, `— in progress` means partially implemented with remaining work listed, and an unmarked section makes no completion claim. Completed sections describe implemented capabilities; in-progress sections state completed work and then list remaining tasks; unmarked sections list requirements only.

## Status baseline

- Roadmap item 0.1 is complete: the target contract is documented in `C99_IMPLEMENTATION.md`, and `__STDC_HOSTED__` follows hosted versus freestanding driver mode.
- Roadmap item 0.2 is complete for the currently implemented language subset: lexer overflow is diagnosed, parser EOF access is bounded, implemented semantic constraints produce source-spanned diagnostics, preprocessing conditionals and `#error` are diagnosed, and negative regressions cover these paths.
- Roadmap item 0.3 is complete for the current subset: categorized valid/invalid/diagnostic/preprocessor/runtime fixtures are exercised by `tests/conformance.rs`, optional integer differential testing is available against the host compiler, and warning/sanitizer commands are documented.
- Roadmap item 1.1 is complete for the current translation pipeline: newlines are normalized, trigraphs and backslash-newline splices are processed before directives, comments become whitespace while retaining newlines, malformed comments/literals are diagnosed, preprocessing tokens are independently tokenized, and UCNs are accepted in identifiers and literals.
- Roadmap item 1.2 is complete for the current supported preprocessor subset: macro replacement is token-based with recursive-expansion suppression, stringification, token pasting, variadics, `#if` evaluation, macro-expanded includes, `#undef`, `#line`, `_Pragma`, required predefined macros, and conditional/redefinition diagnostics. Full C99 corner-case recovery and expression grammar remain future conformance work.
- Roadmap item 1.3 is in progress: C99 keyword tokens, integer radix/suffix spelling recognition, decimal and hexadecimal floating-constant tokenization (including imaginary suffixes), ellipsis, all ordinary C punctuators (including `#`/`##`), and bracket/brace digraphs are covered. Integer type selection/ranges beyond the signed internal value, complete escape decoding, wide literals, multi-character constants, and precise malformed-number validation still need implementation.
- Roadmap item 2.1 is complete for the current target and supported subset: C99 declaration specifiers, declarators, function pointers, adjusted-parameter metadata, and declaration diagnostics are implemented, with the declaration long-tail tracked in Priority 9.
- Current compiler tests pass, but they cover only part of the language.
- The compiler has partial global-object, floating-point, `_Complex`, and target ABI support, but no complete type checker or C99 preprocessor; `_Imaginary` is diagnosed as unsupported.
- Existing behavior should be preserved only where it is covered by new conformance tests; several current tests encode stubs rather than C99 behavior.

## Priority 0 — Make the compiler safe and diagnostically trustworthy

These items block every later milestone. The compiler must not silently miscompile or crash on ordinary invalid input.

### 0.1 Establish a target and implementation contract — done

Complete for the current target. The target contract is documented in `C99_IMPLEMENTATION.md`, including:

- The Linux x86-64 System V ABI, LP64 data model, and ELF64 object format.
- Sizes, alignments, integer representations, plain-`char` signedness, floating formats, `wchar_t`, `size_t`, `ptrdiff_t`, startup behavior, supported extensions, and implementation-defined choices.
- The implementation-defined behavior required by C99 4.8 and Annex J.3 for the current implementation.
- Hosted and freestanding modes, with `__STDC_HOSTED__` defined as `1` and `0` respectively.

### 0.2 Replace panics and silent acceptance with diagnostics — done

Complete for the currently implemented language subset. Invalid input in the supported language is diagnosed instead of panicking or being silently accepted:

- Lexer numeric accumulation is checked, and out-of-range constants are diagnosed rather than panicking.
- Parser recovery is bounded at end-of-file.
- Implemented constraint violations produce source-spanned diagnostics for invalid lvalues, incompatible assignments, invalid calls, invalid returns, invalid member access, invalid pointer operations, and invalid object types.
- Source locations are preserved through preprocessing and macro expansion.
- Negative regressions assert that every required diagnostic in the supported subset is emitted.

### 0.3 Build a conformance-test harness — done

Complete for the current subset. The conformance harness provides:

- Separate fixtures for accepted valid programs, rejected constraint violations, required diagnostics, preprocessor behavior, and runtime behavior.
- Preprocessor output comparison against a reference implementation where practical.
- An optional differential test mode against GCC or Clang for well-defined integer programs.
- Documented warning and sanitizer commands.
- Retained smoke tests that are no longer treated as language coverage.

## Priority 1 — Implement the translation pipeline correctly

### 1.1 Implement C99 translation phases 1–3 — done

Complete for the current translation pipeline:

- Source characters and newlines are normalized.
- Trigraph replacement is implemented.
- Backslash-newline splicing is performed before directive and token processing.
- Source-file termination is enforced, and partial preprocessing tokens and comments are rejected.
- Preprocessing tokens are tokenized separately from C tokens.
- Comments become whitespace while retaining newlines.
- Universal character names are supported in identifiers, character constants, and string literals.

### 1.2 Replace the line-based preprocessor — done

Complete for the current supported preprocessor subset. The preprocessor is token-based and provides:

- Object-like and function-like macros with correct token boundaries.
- Recursive macro expansion and disabled-macro rules.
- `#`, `##`, variadic macros, empty arguments, and argument prescan.
- `#if` expressions with integer constants, `defined`, unary/binary operators, and macro expansion.
- `#include`, including macro-expanded header names and nested relative includes.
- `#undef` and correct macro redefinition rules.
- `#line`, `#error`, null directives, and `_Pragma`.
- Conditional-inclusion error checking for unmatched or repeated `#else`/`#endif`.
- Required predefined macros: `__DATE__`, `__FILE__`, `__LINE__`, `__STDC__`, `__STDC_HOSTED__`, `__STDC_VERSION__`, and `__TIME__`.
- Correct include guards and repeated standard-header inclusion semantics.

The implementation keeps `#pragma once` as an extension without using it as a substitute for standard preprocessing behavior. Coverage beyond this subset, including full C99 corner-case recovery and expression grammar, is future conformance work.

### 1.3 Implement the complete C99 lexer — in progress

In progress for the current lexer subset. All C99 keyword tokens are present, typedef names are ordinary identifiers rather than lexer keywords, integer radix and suffix spellings are recognized, decimal and hexadecimal floating constants (including imaginary suffixes) are tokenized, and ellipsis, all ordinary C punctuators (including `#` and `##`), and bracket/brace digraphs are supported.

Remaining work:

- Select complete integer constant types and ranges instead of using only the signed internal value.
- Implement complete simple, octal, hexadecimal, and universal-character escape-sequence decoding.
- Support wide character and wide string literals.
- Support multi-character constants according to a documented implementation-defined choice.
- Validate malformed numeric constants precisely.

## Priority 2 — Build a real type and declaration system

### 2.1 Redesign declarations and declarators — done

Complete for the current target and supported subset. The parser handles C99 declaration-specifier and declarator forms, including:

- Arbitrary declaration-specifier ordering, storage classes, and type qualifiers.
- Multiple declarators per declaration.
- Nested declarators, including function-returning-pointer, function-pointer, and array-of-function-pointer declarations.
- Function declarators and prototype parameter lists (named and abstract parameters, multiple function declarators, unspecified lists, variadic declarations, and old-style definitions).
- Function pointer declarations.
- Array qualifiers, `static` array bounds, and variable-length arrays.
- Type names for casts, `sizeof`, and compound literals.
- `extern`, `static`, `auto`, `register`, and `inline`.
- Block-scope typedefs and typedef-name disambiguation.
- Static/qualifier metadata on adjusted parameter types.
- Diagnostics for incomplete return/parameter types, misapplied `restrict`, qualified or static arrays outside parameters, and static bounds without sizes.

Long-tail declaration coverage is tracked in Priority 9.

### 2.2 Implement scopes, namespaces, linkage, and tags — done for the supported subset

Ordinary, tag, and member namespaces are separated. Typedef names and enumerators share ordinary-identifier behavior, including shadowing. File, block, function, and function-prototype scopes are tracked; tags declared in prototype parameter lists are hidden afterward and remain visible in definitions. Tag declarations resolve forward and completed types, with duplicate definitions diagnosed. File-scope linkage now distinguishes external and internal linkage, inherits internal linkage through compatible redeclarations, and diagnoses static/non-static conflicts. Compatible declarations merge, incompatible declarations are diagnosed, and function definitions are tracked separately from declarations.

Remaining work:

- Add label and `goto` namespaces when labels and `goto` are represented in the AST (tracked with the AST gaps in Priority 9).
- Cover long-tail namespace diagnostics for conflicting ordinary identifiers.

### 2.3 Implement all C99 types — done for the supported subset

Complete for the current target and supported subset. `void`, `_Bool`, character and integer types, floating types, `_Complex`, arrays, functions, pointers, structures, unions, enumerations, qualified types, and bit-fields are modeled. `_Imaginary` is parsed and rejected with a diagnostic rather than silently accepted.

Incomplete object and function types are rejected except where declarations are explicitly allowed to refer to incomplete arrays or aggregate types. Compatible file-scope redeclarations merge using qualifier-aware composite types, including old-style/prototype function merges and tentative incomplete arrays completed to one element.

`_Imaginary` remains intentionally rejected, and long-tail compatibility cases are tracked with the remaining type-system work in Priority 9.

### 2.4 Add target layout and representation — in progress

In progress for the current target. Size, alignment, field offsets, and union extent for x86-64 Linux are centralized in `src/layout.rs`; aggregate initializers, including brace elision and designated items, use the same layout. Data emission writes aggregate bytes and relocations against the resulting layout. Bit-field offsets, widths, masks, and storage-unit layout are shared with the initializer and code-generation paths. Floating constants and `_Complex` objects with `float` and `double` bases are represented and emitted in static data.

Remaining work:

- Implement `_Bool` normalization.
- Implement `long double`'s distinct x87 representation.
- Complete complex and aggregate calling-convention coverage.
- Ensure generated ABI types match declared C types in all supported cases.

## Priority 3 — Implement semantic analysis and conversions

### 3.1 Implement lvalue and modifiability rules

- Validate assignment targets.
- Reject assignments to const-qualified objects.
- Validate address-of and dereference operands.
- Validate increment/decrement operands.
- Validate member and arrow operands and member names.
- Validate array subscripting operands.

### 3.2 Implement C99 conversions

- Lvalue, array-to-pointer, and function-designator conversions.
- Integer promotions.
- Usual arithmetic conversions.
- Integer, floating, pointer, and qualification conversions.
- Null pointer constants.
- Assignment, parameter, and return conversions.
- Conditional-operator conversions.
- Pointer arithmetic and pointer subtraction constraints.

### 3.3 Implement constant expressions — in progress

In progress for the current target and supported subset. Integer and floating constant expressions are evaluated for static initializers.

Remaining work:

- Extend the evaluator to every required context, including enum values, array bounds, bit-field widths, case labels, and `#if`.
- Diagnose invalid constant expressions and representability failures uniformly.

## Priority 4 — Complete expressions and statements

### 4.1 Expressions — in progress

In progress for the current target and supported subset. Function designators and indirect calls are implemented, including function-to-pointer assignment and calls through function pointers. Floating and complex arithmetic, comparisons, conversions, and negation are implemented.

Remaining work:

- Implement casts.
- Implement `sizeof` for expressions and type names.
- Implement compound literals.
- Implement correct prefix and postfix increment/decrement semantics.
- Implement pointer comparisons and pointer difference.
- Preserve C evaluation and sequencing rules.
- Ensure short-circuit operators and conditional expressions evaluate only the permitted operand.
- Ensure compound assignments evaluate the left operand exactly once, and complete compound-assignment coverage.

### 4.2 Initializers

- Scalar initializers with conversions.
- Array and structure/union initializer lists.
- Designators such as `[index] = value` and `.member = value`.
- String-literal initialization of character arrays.
- Static-storage initialization and zero initialization.
- Variable-length-array initialization constraints.

### 4.3 Statements

- Labeled statements, `goto`, and label resolution.
- `switch`, `case`, and `default`, including duplicate-case diagnostics.
- Null statements.
- Correct declaration scope in `for` statements.
- Correct `continue` targets for `while`, `do`, and `for`.
- Correct function-end behavior: reaching `}` in `main` returns zero; doing so in another value-returning function has the C99-defined behavior rather than an unconditional generated zero.

## Priority 5 — Correct code generation and ABI integration

### 5.1 Replace the integer-only QBE lowering — in progress

In progress for the current target. Lowering and code generation emit typed `w`, `l`, `s`, and `d` values for integer, pointer, floating, and `_Complex` operations, including conversions, comparisons, returns, and typed call results. Aggregate and complex calls use QBE aggregate types where implemented.

Remaining work:

- Complete signed and unsigned division, remainder, comparison, and shift coverage.
- Load and store all integer widths with correct sign extension.
- Implement aggregate argument and return ABI rules or explicitly reject unsupported ABI cases with diagnostics.
- Complete unsigned conversion variants and `long double`'s distinct representation.

### 5.2 Implement objects and storage

- Emit file-scope objects and initializers.
- Emit static-duration locals.
- Emit read-only and writable data with correct alignment.
- Implement address constants and relocations.
- Implement automatic-object allocation with correct size and alignment.
- Keep lexical scopes separate in lowering so shadowed variables receive distinct storage.

### 5.3 Implement function/linkage behavior — in progress

In progress for the current target and supported subset. Function pointers and indirect calls are implemented for expressions, parameters, aggregates, and file-scope initializers.

Remaining work:

- Respect `static` and external linkage.
- Support declarations in one translation unit and definitions in another.
- Diagnose incompatible declarations and duplicate definitions.
- Verify hosted `main` forms and freestanding startup behavior against the documented target contract.

## Priority 6 — Complete the required freestanding headers

### 6.1 `<float.h>`

Provide all required C99 floating characteristics, including `FLT_DIG`, `DBL_DIG`, `LDBL_DIG`, decimal exponent limits, minimum/maximum values, epsilon values, `FLT_ROUNDS`, and `FLT_EVAL_METHOD`. Values must match the compiler’s actual floating implementation.

### 6.2 `<iso646.h>`

Keep the existing eleven alternative-spelling macros and add regression tests for each one.

### 6.3 `<limits.h>`

Provide all required limits, including `CHAR_MIN`, `CHAR_MAX`, `MB_LEN_MAX`, `LLONG_MIN`, `LLONG_MAX`, and `ULLONG_MAX`. Ensure values are valid constant expressions and match the target data model.

### 6.4 `<stdarg.h>` — in progress

In progress for the current target and supported subset. Scalar integer, pointer, and floating variadic arguments work through QBE `vastart`/`vaarg`.

Remaining work:

- Provide complete `va_list`, `va_start`, `va_arg`, `va_copy`, and `va_end` behavior.
- Implement target-ABI variable-argument lowering for aggregate arguments.
- Add tests for aggregate variadic arguments and any remaining scalar cases.

### 6.5 `<stdbool.h>`

Retain the required macros, but implement `_Bool` conversions and storage semantics correctly.

### 6.6 `<stddef.h>`

- Define target-correct `size_t`, `ptrdiff_t`, and `wchar_t`.
- Implement `offsetof` using actual structure layout.
- Test the required null-pointer and type behavior.

### 6.7 `<stdint.h>`

- Implement exact-width types only when the target provides them.
- Add least-width, fast-width, pointer-width, and greatest-width types.
- Add all required limit macros.
- Add `INTN_C` and `UINTN_C` macros.
- Ensure all types and macro values match the target ABI.

## Priority 7 — Conformance polish and tooling

- Add `-std=c99` or an equivalent documented language-mode option.
- Add extension diagnostics and a policy for unsupported extensions.
- Produce standards-based diagnostics with source locations after macro expansion.
- Add preprocessor-only, compile-only, assembly, object, link, and runtime conformance suites.
- Add translation-limit tests for every minimum in C99 5.2.4.1.
- Add tests for all required implementation-defined choices.
- Add a generated conformance report to CI.
- Document unsupported optional Annexes rather than implying full C99 support.
- Keep hosted libc tests separate from core language tests; do not let the local stub headers mask failures in the compiler’s glibc integration.

## Priority 9 — Lowest-priority declaration long-tail

Deferred coverage from 2.1:

- Exhaustive declaration-specifier ordering and permutation combinations.
- Remaining nested declarator combinations and their conformance cases.
- Remaining old-style parameter declaration constraints and ABI conversions.
- Exotic function declaration constraints not yet diagnosed.

## Definition of done

The project should not claim C99 coverage until it can demonstrate all of the following:

1. Every required C99 syntax form parses or receives a required diagnostic.
2. Every constraint violation in the supported language receives at least one diagnostic.
3. Every accepted well-defined test has correct behavior on the documented target.
4. The seven required freestanding headers are complete and ABI-consistent.
5. Translation phases, preprocessing, declarations, types, conversions, initialization, statements, linkage, and code generation are covered by independent tests.
6. Implementation-defined behavior is documented.
7. No compiler panic occurs for malformed or out-of-range C99 input.

# Roadmap to C99 Coverage

This roadmap is ordered by urgency. It treats hosted libc implementation as out of scope because glibc is provided externally, but it includes the seven freestanding headers required by C99 4 and the compiler support needed to consume them.

The target is a documented, test-backed C99 implementation for one declared target ABI. “Coverage” means accepting valid C99 programs, rejecting constraint violations with diagnostics, and producing behavior consistent with the selected implementation-defined choices. It does not require optional Annex F IEC 60559 conformance unless explicitly claimed.

## Status baseline

- Roadmap item 0.1 is complete: the target contract is documented in `C99_IMPLEMENTATION.md`, and `__STDC_HOSTED__` follows hosted versus freestanding driver mode.
- Roadmap item 0.2 is complete for the currently implemented language subset: lexer overflow is diagnosed, parser EOF access is bounded, implemented semantic constraints produce source-spanned diagnostics, preprocessing conditionals and `#error` are diagnosed, and negative regressions cover these paths.
- Roadmap item 0.3 is complete for the current subset: categorized valid/invalid/diagnostic/preprocessor/runtime fixtures are exercised by `tests/conformance.rs`, optional integer differential testing is available against the host compiler, and warning/sanitizer commands are documented.
- Roadmap item 1.1 is complete for the current translation pipeline: newlines are normalized, trigraphs and backslash-newline splices are processed before directives, comments become whitespace while retaining newlines, malformed comments/literals are diagnosed, preprocessing tokens are independently tokenized, and UCNs are accepted in identifiers and literals.
- Roadmap item 1.2 is implemented for the current supported preprocessor subset: macro replacement is token-based with recursive-expansion suppression, stringification, token pasting, variadics, `#if` evaluation, macro-expanded includes, `#undef`, `#line`, `_Pragma`, required predefined macros, and conditional/redefinition diagnostics. Full C99 corner-case recovery and expression grammar remain follow-up work.
- Roadmap item 1.3 is in progress: C99 keyword tokens, integer radix/suffix spelling recognition, decimal and hexadecimal floating-constant tokenization (including imaginary suffixes), ellipsis, all ordinary C punctuators (including `#`/`##`), and bracket/brace digraphs are covered. Integer type selection/ranges beyond the signed internal value, complete escape decoding, wide literals, multi-character constants, and precise malformed-number validation still need implementation.
- Current compiler tests pass, but they cover only part of the language.
- The compiler has partial global-object, floating-point, `_Complex`, and target ABI support, but no complete type checker or C99 preprocessor; `_Imaginary` is diagnosed as unsupported.
- Existing behavior should be preserved only where it is covered by new conformance tests; several current tests encode stubs rather than C99 behavior.

## Priority 0 — Make the compiler safe and diagnostically trustworthy

These items block every later milestone. The compiler must not silently miscompile or crash on ordinary invalid input.

### 0.1 Establish a target and implementation contract

- Choose and document the initial target ABI, currently apparently Linux x86-64 System V.
- Document sizes, alignments, integer representations, plain-`char` signedness, floating formats, `wchar_t`, `size_t`, `ptrdiff_t`, startup behavior, supported extensions, and implementation-defined choices.
- Add a `C99_IMPLEMENTATION.md` section describing all implementation-defined behavior required by C99 4.8 and Annex J.3.
- Decide whether the implementation is hosted, freestanding, or both, and define the `__STDC_HOSTED__` value accordingly.

### 0.2 Replace panics and silent acceptance with diagnostics

- Make lexer numeric accumulation checked; report out-of-range constants instead of panicking.
- Ensure parser recovery never indexes beyond end-of-file.
- Add diagnostics for every implemented constraint violation, including invalid lvalues, incompatible assignments, invalid calls, invalid returns, invalid member access, invalid pointer operations, and invalid object types.
- Preserve source locations through preprocessing and macro expansion.
- Add negative tests asserting that every required diagnostic is emitted.

### 0.3 Build a conformance-test harness

- Add separate tests for: accepted valid programs, rejected constraint violations, required diagnostics, and runtime behavior.
- Compare preprocessor output against a reference implementation where practical.
- Add a differential test mode against GCC or Clang for well-defined integer programs.
- Run tests under sanitizers and with compiler warnings enabled.
- Keep the current smoke tests, but stop treating them as language coverage.

## Priority 1 — Implement the translation pipeline correctly

### 1.1 Implement C99 translation phases 1–3

- Normalize source characters and newlines.
- Implement trigraph replacement.
- Implement backslash-newline splicing before directive and token processing.
- Enforce source-file termination and reject partial preprocessing tokens/comments.
- Tokenize preprocessing tokens separately from C tokens.
- Replace comments with whitespace while retaining newlines.
- Support universal character names in identifiers, character constants, and string literals.

### 1.2 Replace the line-based preprocessor

Implement a token-based preprocessor covering:

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

Keep `#pragma once` as an extension, but do not use it as a substitute for standard preprocessing behavior.

### 1.3 Implement the complete C99 lexer

- Add all C99 keywords, including `auto`, `case`, `default`, `extern`, `goto`, `register`, `switch`, `inline`, `_Complex`, and `_Imaginary`.
- Stop hard-coding typedef names such as `size_t` and `int32_t` as lexer keywords.
- Add decimal, octal, hexadecimal, and suffixed integer constants with correct type selection.
- Add decimal and hexadecimal floating constants with suffixes.
- Implement all simple, octal, hexadecimal, and universal-character escape sequences.
- Support wide character and wide string literals.
- Support multi-character constants according to a documented implementation-defined choice.
- Add `...` and all C99 punctuators.

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

### 2.2 Implement scopes, namespaces, linkage, and tags

- Maintain separate ordinary, tag, member, and label namespaces.
- Give typedef names the correct ordinary-identifier behavior.
- Implement file, block, function, and function-prototype scopes.
- Resolve forward declarations and completed `struct`, `union`, and `enum` tags.
- Implement external, internal, and no-linkage rules.
- Merge compatible declarations and diagnose incompatible declarations.
- Track function declarations separately from function definitions.

### 2.3 Implement all C99 types

- Correctly model `void`, `_Bool`, character types, signed/unsigned integer types, floating types, `_Complex`, and `_Imaginary` where supported.
- Model arrays of known and unknown bound, functions, pointers, structures, unions, enumerations, and qualified types.
- Add bit-fields.
- Track incomplete types and reject invalid object/function uses.
- Add compatibility and composite-type computation.

Progress: floating and `_Complex` types are modeled for `float`, `double`, and `long double` bases, including arithmetic, comparisons, conversions, negation, function parameters/returns, block and file-scope initialization (including designated aggregate members), and `sizeof`. `_Imaginary` is parsed and rejected with a diagnostic rather than silently accepted. Incomplete-type and composite-type handling remain incomplete.

### 2.4 Add target layout and representation

Progress: size, alignment, field offsets, and union extent for the x86-64 Linux target are centralized in `src/layout.rs`; aggregate initializers, including brace elision and designated items, use the same layout. Data emission writes aggregate bytes and relocations against the resulting layout. Bit-field offsets, widths, masks, and storage-unit layout are shared with the initializer and code-generation paths. Floating constants and `_Complex` objects (`float` and `double` bases) are represented and emitted in static data. `_Bool` normalization, `long double`'s distinct x87 representation, and complete complex calling-convention coverage remain outstanding.

- Centralize size, alignment, rank, and representation rules in a target data-layout module.
- Correctly lay out structure padding and alignment.
- Make union size/alignment the maximum required member layout.
- Implement signed loads/stores and correct `_Bool` normalization.
- Define and test integer widths and overflow behavior.
- Ensure generated ABI types match declared C types.

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

### 3.3 Implement constant expressions

- Build an integer constant-expression evaluator.
- Use it for enum values, array bounds, bit-field widths, case labels, static initializers, and `#if` where appropriate.
- Integer and floating constant expressions are evaluated for static initializers; extend the evaluator to all required contexts and diagnose representability failures uniformly.
- Detect invalid constant expressions and representability failures.

## Priority 4 — Complete expressions and statements

### 4.1 Expressions

- Implement casts.
- Implement `sizeof` for expressions and type names.
- Implement compound literals.
- Implement correct prefix and postfix increment/decrement semantics.
- Function designators and indirect calls are implemented, including function-to-pointer assignment and calls through function pointers.
- Floating and complex arithmetic, comparisons, conversions, and negation are implemented; complete evaluation-order and compound-assignment coverage remains outstanding.
- Implement pointer comparisons and pointer difference.
- Preserve C evaluation and sequencing rules.
- Ensure short-circuit operators and conditional expressions evaluate only the permitted operand.
- Ensure compound assignments evaluate the left operand exactly once.

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

### 5.1 Replace the integer-only QBE lowering

Progress: lowering and code generation now emit typed `w`, `l`, `s`, and `d` values for integer, pointer, floating, and `_Complex` operations, including conversions, comparisons, returns, and typed call results. Aggregate and complex calls use QBE aggregate types where implemented. Full aggregate argument/return ABI edge cases, unsigned conversion variants, and `long double`'s distinct representation remain outstanding.

- Emit QBE `w`, `l`, `s`, and `d` values according to C types.
- Generate signed and unsigned division, remainder, comparisons, and shifts correctly.
- Generate floating arithmetic, comparisons, conversions, and returns.
- Preserve pointer widths and pointer arithmetic.
- Load and store all integer widths with correct sign extension.
- Emit correctly typed call results; current calls always use a `w` result.
- Implement aggregate argument/return ABI rules or explicitly reject unsupported ABI cases with diagnostics.

### 5.2 Implement objects and storage

- Emit file-scope objects and initializers.
- Emit static-duration locals.
- Emit read-only and writable data with correct alignment.
- Implement address constants and relocations.
- Implement automatic-object allocation with correct size and alignment.
- Keep lexical scopes separate in lowering so shadowed variables receive distinct storage.

### 5.3 Implement function/linkage behavior

- Respect `static` and external linkage.
- Support declarations in one translation unit and definitions in another.
- Diagnose incompatible declarations and duplicate definitions.
- Function pointers and indirect calls are implemented for expressions, parameters, aggregates, and file-scope initializers.
- Verify hosted `main` forms and freestanding startup behavior against the documented target contract.

## Priority 6 — Complete the required freestanding headers

### 6.1 `<float.h>`

Provide all required C99 floating characteristics, including `FLT_DIG`, `DBL_DIG`, `LDBL_DIG`, decimal exponent limits, minimum/maximum values, epsilon values, `FLT_ROUNDS`, and `FLT_EVAL_METHOD`. Values must match the compiler’s actual floating implementation.

### 6.2 `<iso646.h>`

Keep the existing eleven alternative-spelling macros and add regression tests for each one.

### 6.3 `<limits.h>`

Provide all required limits, including `CHAR_MIN`, `CHAR_MAX`, `MB_LEN_MAX`, `LLONG_MIN`, `LLONG_MAX`, and `ULLONG_MAX`. Ensure values are valid constant expressions and match the target data model.

### 6.4 `<stdarg.h>`

- Implement variadic function declarators and definitions.
- Implement target-ABI variable argument lowering.
- Provide working `va_list`, `va_start`, `va_arg`, `va_copy`, and `va_end` behavior.
- Add tests for integer, pointer, floating, and aggregate variadic arguments.

Progress: scalar integer, pointer, and floating variadic arguments work through QBE `vastart`/`vaarg`; aggregate `va_arg` remains unsupported.

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


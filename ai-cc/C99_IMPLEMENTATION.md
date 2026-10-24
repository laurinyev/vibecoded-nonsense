# ai-cc C99 Implementation Contract

This document records the implementation-defined choices for the current `ai-cc` target. It is a target contract, not a claim that the compiler is already C99-conforming. Unsupported and incomplete areas remain tracked in [`C99_ROADMAP.md`](C99_ROADMAP.md).

## Conformance modes

`ai-cc` currently has two driver modes:

- Hosted mode is the default. It targets Linux x86-64 and links with the host C runtime and glibc.
- `--freestanding` uses the project CRT, starts at `_start`, makes no libc or host-startup dependency, and terminates through the Linux `exit` syscall.

The compiler currently defines `__STDC_HOSTED__` as `1` in hosted mode and `0` in `--freestanding` mode. This is the first mode-dependent predefined macro; the remaining C99 predefined macros are tracked as preprocessor work in the roadmap.

The project does not currently claim full C99 conformance. It does not claim Annex F IEC 60559 conformance, Annex G complex arithmetic conformance, or any compiler-specific extension compatibility beyond the behavior documented here.

## Target platform and ABI

The supported target is:

- Operating system: Linux.
- Architecture: x86-64.
- Object format: ELF64.
- ABI: System V AMD64.
- Data model: LP64.
- Endianness: little-endian.
- Assembler/linker: GNU-compatible `as` and `ld`, or compatible tools selected through the documented `AI_CC_AS` and `AI_CC_LD` variables.
- Backend: QBE configured for the host target.

The current freestanding CRT is x86-64-specific. Other architectures are not supported by this contract even though the driver contains limited dynamic-linker probing for other host names.

The intended System V AMD64 calling convention is:

- Integer and pointer arguments use the standard AMD64 integer argument registers (`RDI`, `RSI`, `RDX`, `RCX`, `R8`, `R9`) followed by the ABI stack area.
- Floating arguments use the ABI floating-point argument registers when floating support is implemented.
- Integer and pointer results are returned in `RAX`; floating results use the ABI floating-point result register.
- Callee-saved registers, stack alignment, aggregate passing, aggregate returns, and variadic arguments follow the System V AMD64 ABI.

The backend does not yet implement every part of this intended ABI. In particular, aggregate layout, floating values, variadic arguments, and several integer widths remain roadmap work.

## C object and scalar model

The intended data model is:

| C type | Size | Alignment | Representation/choice |
|---|---:|---:|---|
| `char` | 1 byte | 1 | Plain `char` is signed; execution bytes use 8-bit units. |
| `signed char` | 1 byte | 1 | Two’s-complement signed integer. |
| `unsigned char` | 1 byte | 1 | Pure-binary unsigned integer. |
| `_Bool` | 1 byte | 1 | Values normalize to 0 or 1. |
| `short` / `unsigned short` | 2 bytes | 2 | Two’s-complement signed / pure-binary unsigned. |
| `int` / `unsigned int` | 4 bytes | 4 | Two’s-complement signed / pure-binary unsigned. |
| `long` / `unsigned long` | 8 bytes | 8 | Two’s-complement signed / pure-binary unsigned. |
| `long long` / `unsigned long long` | 8 bytes | 8 | Two’s-complement signed / pure-binary unsigned. |
| `float` | 4 bytes | 4 | IEC 60559 binary32 representation is intended. |
| `double` | 8 bytes | 8 | IEC 60559 binary64 representation is intended. |
| `long double` | 16-byte storage | 16 | x86 extended precision value padded in a 16-byte object. |
| Object/function pointers | 8 bytes | 8 | Flat 64-bit address representation. |

Additional choices:

- `CHAR_BIT` is 8.
- Signed integer arithmetic uses two’s-complement representation. Signed overflow remains undefined behavior; unsigned arithmetic is modulo 2 to the width.
- `float` has radix 2 and 24 bits of precision; `double` has radix 2 and 53 bits of precision; `long double` has radix 2 and 64 bits of significand precision.
- The intended floating evaluation method is 0: expressions and constants are evaluated in their nominal type format, subject to the eventual backend implementation.
- Structure members are aligned according to their member type, with padding inserted as required; structure size is rounded to its alignment.
- Union size is the maximum member size rounded to the union alignment.
- Enumeration types use an implementation-selected integer type capable of representing all enumerators; the initial target uses `int` where possible.
- A null pointer is represented by an all-zero pointer value for the initial target.

These choices are the contract to implement. The current compiler does not yet satisfy all of them; for example, its backend currently collapses most scalar values to QBE `w` values and does not implement structure padding.

## Fundamental typedef choices

The intended initial-target definitions are:

- `size_t`: unsigned 64-bit integer, ABI-compatible with `unsigned long`.
- `ptrdiff_t`: signed 64-bit integer, ABI-compatible with `long`.
- `wchar_t`: signed 32-bit integer. Wide-character encoding and locale behavior remain incomplete.
- `int8_t`, `int16_t`, `int32_t`, `int64_t`: exact-width signed integers.
- `uint8_t`, `uint16_t`, `uint32_t`, `uint64_t`: exact-width unsigned integers.
- `intmax_t`: signed 64-bit integer.
- `uintmax_t`: unsigned 64-bit integer.

The corresponding `least`, `fast`, pointer-width, and integer-constant facilities required by `<stdint.h>` are not yet implemented.

## Character sets and literals

The initial target intends:

- The basic source and execution character sets contain the ASCII basic character set.
- Ordinary execution character strings use UTF-8 bytes for non-basic source characters where supported.
- The null character is a zero byte.
- Plain character signedness is signed.

Universal character names, complete escape-sequence handling, wide literals, locale-sensitive multibyte conversion, and full source-character mapping remain unimplemented.

## Program startup and termination

### Hosted mode

Hosted mode uses the host Linux C runtime startup objects and glibc. The intended supported C99 `main` forms are:

```c
int main(void);
int main(int argc, char *argv[]);
```

The host runtime owns process startup, argument construction, static initialization, and normal hosted termination. The compiler must still generate the correct C function ABI and enforce the C99 restrictions on `main` as those features are implemented.

### Freestanding mode

The project CRT defines `_start` and performs the following implementation-defined sequence:

1. Call `main` without C parameters.
2. Move the returned integer value to the Linux `exit` syscall argument.
3. Invoke syscall 60 (`exit`).
4. Halt if control unexpectedly returns from the syscall.

The freestanding startup function is therefore `_start`; `main` is a project convention rather than a C99 freestanding requirement. No glibc startup objects or libc are linked in this mode.

## Predefined macros and extensions

Currently implemented:

- `__STDC_HOSTED__`: `1` in hosted mode, `0` in freestanding mode.
- `#pragma once`: supported as a nonstandard extension.
- `--freestanding`: project driver option.

Not currently defined because the compiler cannot yet claim the corresponding conformance:

- `__STDC__`.
- `__STDC_VERSION__`.
- `__FILE__`, `__LINE__`, `__DATE__`, and `__TIME__`.
- IEC 60559 and ISO 10646 capability macros.

The compiler must not advertise `__STDC__ == 1` or `__STDC_VERSION__ == 199901L` until the required C99 behavior is implemented and tested.

## Library boundary

Hosted library implementation is outside this project’s scope; hosted programs are intended to link against glibc. The seven C99 freestanding headers remain in scope:

```text
<float.h> <iso646.h> <limits.h> <stdarg.h>
<stdbool.h> <stddef.h> <stdint.h>
```

Their required contents must match the target choices above. The current local headers are placeholders and remain tracked in the roadmap.


# ai-cc

The implementation contract and current C99 coverage boundary are documented in
[`C99_IMPLEMENTATION.md`](C99_IMPLEMENTATION.md). The ordered implementation roadmap is in
[`C99_ROADMAP.md`](C99_ROADMAP.md).

Install compiler and freestanding headers:

```sh
./tools/install.sh /usr/local
```

Installed layout:

```text
prefix/bin/ai-cc
prefix/lib/include/*.h
prefix/libexec/ai-cc/qbe
prefix/libexec/ai-cc/crt/crtbegin.o
prefix/libexec/ai-cc/crt/crtend.o
```

Header lookup checks source-relative paths, `AI_CC_INCLUDE`, the build-tree `lib/include`, and installed `prefix/lib/include`.

The driver searches QBE in `AI_CC_QBE`, beside the compiler binary, `prefix/libexec/ai-cc/qbe`, the build-tree `tools/qbe`, and `PATH`. `-E` writes preprocessed C, `-S` writes QBE IR, `-c` writes native object files through QBE and `cc`, and the default mode links an executable.

Default linking uses system `crt1.o`, `crti.o`, and `crtn.o`, plus locally built compiler CRT objects. `AI_CC_AS`, `AI_CC_LD`, `AI_CC_CRT_DIR`, and `AI_CC_DYNAMIC_LINKER` override host tool and CRT discovery. `-v` prints QBE, `as`, and `ld` commands.

`--freestanding` links with compiler `crtstart.o`/`crtend.o`, enters at `_start`, exits through the Linux `exit` syscall, and omits glibc and host startup files.

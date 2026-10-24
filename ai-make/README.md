# ai-make

`ai-make` builds explicit C source lists with the sibling `../ai-cc` compiler.

## Configuration

```yaml
targets:
  hello:
    sources:
      - ../ai-cc/tests/cases/header_stdbool.c
    output: build/hello
    flags: []
    link_flags: []
```

Paths are relative to the configuration file. Source lists are explicit; directory scanning, wildcards, and globbing are not supported.

## Usage

```sh
cargo run -- AiMake.yaml
```

Each run first executes `cargo build --release` in `../ai-cc`, compiles every listed source to an intermediate object, then links the target with `cc`.

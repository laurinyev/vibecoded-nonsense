# Vibecoded Nonsense

A workspace for assorted vibecoded projects—currently a compiler, a tiny build tool, and a game that puts them to work.

## Projects

- [`ai-cc/`](ai-cc/) — an experimental C compiler targeting QBE, with a growing C99 implementation and freestanding support. Its [implementation notes](ai-cc/C99_IMPLEMENTATION.md) and [coverage roadmap](ai-cc/C99_ROADMAP.md) describe current support and gaps.
- [`ai-make/`](ai-make/) — a small Rust build orchestrator that reads YAML target descriptions, builds with the sibling `ai-cc`, and links outputs with the host C toolchain.
- [`grid-runner/`](grid-runner/) — a deterministic terminal game written in C; it serves as an end-to-end example for `ai-cc` and `ai-make`.

## Building

The Rust projects require a Rust toolchain with Cargo. `ai-cc` also needs a host C toolchain and its QBE backend; see [`ai-cc/README.md`](ai-cc/README.md) for installation details and environment overrides. Build the individual tools from their directories:

```sh
cd ai-cc && cargo build --release
cd ../ai-make && cargo build --release
```

To build and run the game through the workspace tools, run from the project directory:

```sh
cd ../grid-runner && ./run.sh
```

## Licensing

This workspace is licensed under GNU General Public License, version 3 only (GPL-3.0-only). The complete license text, obtained from the [Free Software Foundation](https://www.gnu.org/licenses/gpl-3.0.txt), is in [`LICENSE`](LICENSE). Unless a file or subproject says otherwise, the workspace projects are intended to be distributed under these terms.

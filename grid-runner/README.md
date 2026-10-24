# Grid Runner

Deterministic terminal game simulation for exercising `ai-make` and `ai-cc`.

The player navigates a generated grid, collects energy, avoids hazards, and reaches an exit. The run is deterministic so build and runtime checks are reproducible.

Run from this directory:

```sh
./run.sh
```

`run.sh` builds uninstalled workspace binaries from `ai-cc/` and `ai-make/`, then invokes `ai-make` with this project configuration. The configuration lists every source explicitly. No directory discovery or globbing is used.

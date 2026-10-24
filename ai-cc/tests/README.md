# Tests

`compiler.rs` exercises the library frontend, generates QBE, links libc, executes each binary inside Bubblewrap, and checks exact stdout. Set `QBE` to use an existing QBE binary.

Fixtures derive from constructs illustrated by `../c99.pdf`: expression grouping in §5.1.2.3, preprocessing in §5.1, and selection/iteration statements in §6.8. Header cases cover all seven freestanding headers required by §4: `<float.h>`, `<iso646.h>`, `<limits.h>`, `<stdarg.h>`, `<stdbool.h>`, `<stddef.h>`, and `<stdint.h>`. Output uses libc `putchar`.

Run:

```sh
cargo test --test compiler
./tests/driver.sh
```

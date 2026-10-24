#include <stdbool.h>
int main() {
    bool enabled = true;
    if (enabled && !false) {
        putchar(66);
    }
    putchar(10);
    return 0;
}

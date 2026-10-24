#include <stdbool.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdint.h>
int main() {
    bool enabled = true;
    int32_t value = INT32_MAX;
    if (enabled && value > 0) {
        putchar(82);
    }
    putchar(10);
    return 0;
}

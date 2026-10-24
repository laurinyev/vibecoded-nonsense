#include <stddef.h>
int main() {
    size_t value = 3;
    ptrdiff_t delta = -1;
    wchar_t letter = 65;
    if (value > 0 && delta < 0 && letter == 65 && offsetof(int, value) == 0) {
        putchar(68);
    }
    putchar(10);
    return 0;
}

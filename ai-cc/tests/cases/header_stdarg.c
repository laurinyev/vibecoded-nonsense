#include <stdarg.h>
int main() {
    va_list ap;
    va_start(ap, ap);
    va_arg(ap, int);
    va_copy(ap, ap);
    va_end(ap);
    putchar(65);
    putchar(10);
    return 0;
}

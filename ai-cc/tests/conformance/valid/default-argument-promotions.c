#include <stdarg.h>

enum Number {
    FOUR = 4
};

int promoted_sum(int count, ...) {
    va_list arguments;
    va_start(arguments, count);
    int total = va_arg(arguments, int);
    total += va_arg(arguments, int);
    total += va_arg(arguments, int);
    total += (int)(va_arg(arguments, double) * 4.0);
    va_end(arguments);
    return total;
}

int main(void) {
    signed char narrow = -2;
    unsigned short small = 300;
    enum Number number = FOUR;
    float fraction = 0.5f;
    return promoted_sum(4, narrow, small, number, fraction) != 304;
}

#include <stdarg.h>

int sum(int count, ...) {
    va_list arguments;
    va_start(arguments, count);
    int total = 0;
    for (int i = 0; i < count; i++) {
        total += va_arg(arguments, int);
    }
    va_end(arguments);
    return total;
}

int sum_twice(int count, ...) {
    va_list first;
    va_list second;
    va_start(first, count);
    va_copy(second, first);
    int total = 0;
    for (int i = 0; i < count; i++) {
        total += va_arg(first, int);
    }
    for (int i = 0; i < count; i++) {
        total += va_arg(second, int);
    }
    va_end(second);
    va_end(first);
    return total;
}

double sum_doubles(int count, double initial, ...) {
    va_list arguments;
    va_start(arguments, initial);
    double total = initial;
    for (int i = 0; i < count; i++) {
        total += va_arg(arguments, double);
    }
    va_end(arguments);
    return total;
}

int first_pointer(int count, ...) {
    va_list arguments;
    va_start(arguments, count);
    int *value = va_arg(arguments, int *);
    va_end(arguments);
    return *value;
}

int main(void) {
    int value = 9;
    if (sum(3, 1, 2, 3) != 6) return 1;
    if (sum(0) != 0) return 2;
    if (sum_twice(3, 1, 2, 3) != 12) return 3;
    if (sum_doubles(2, 1.0, 2.0, 3.0) != 6.0) return 4;
    if (first_pointer(1, &value) != 9) return 5;
    return 0;
}

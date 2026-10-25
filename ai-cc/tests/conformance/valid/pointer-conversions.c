int read_value(const int *value) {
    return *value;
}

void *identity(void *value) {
    return value;
}

int *null_pointer(void) {
    return 1 - 1;
}

int is_null(int *pointer) {
    return pointer == 0;
}

int main(void) {
    int value = 7;
    int *pointer = &value;
    const int *qualified = pointer;
    const int *common_qualified = 1 ? pointer : qualified;
    void *generic = pointer;
    int *empty = null_pointer();
    int *initialized_null = 0;

    if (read_value(pointer) != 7) return 1;
    if (*common_qualified != 7) return 10;
    if (pointer != qualified || pointer != generic) return 2;
    if (pointer < qualified) return 3;
    if (pointer == 0 || empty != 0 || initialized_null != 0) return 4;
    if (!is_null(1 - 1)) return 11;
    pointer = 0;
    pointer = &value;
    if ((pointer ? pointer : 0) != pointer) return 5;
    pointer = identity(pointer);
    generic = identity(generic);
    pointer = generic;
    if (*pointer != 7) return 6;

    long signed_value = -1;
    unsigned int small_unsigned = 1;
    if (signed_value < small_unsigned) {
        signed_value += small_unsigned;
    } else {
        return 7;
    }
    if (signed_value != 0) return 8;

    long long signed_wide = -1;
    unsigned long unsigned_wide = 1;
    if (signed_wide < unsigned_wide) return 12;

    unsigned long wide_shift = 1;
    if ((-1 >> wide_shift) != -1) return 9;
    return 0;
}

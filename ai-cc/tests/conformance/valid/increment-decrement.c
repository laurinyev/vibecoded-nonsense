struct Bits {
    unsigned int value : 4;
    signed int signed_value : 3;
};

int main(void) {
    int value = 1;
    int values[3] = { 0, 0, 0 };
    int index = 0;
    int *pointer = values;
    struct Bits bits = { 0 };
    float fraction = 1.5f;
    signed char narrow = 127;

    if (value++ != 1 || value != 2) return 1;
    if (++value != 3 || value != 3) return 2;
    values[index++]++;
    if (index != 1 || values[0] != 1) return 3;
    if (++*pointer != 2 || values[0] != 2) return 4;
    if (pointer++ != values || pointer != values + 1) return 5;
    if (bits.value++ != 0 || bits.value != 1) return 6;
    if (fraction++ != 1.5f || fraction != 2.5f) return 7;
    if (++narrow != -128) return 8;
    bits.signed_value = 3;
    if (++bits.signed_value != -4) return 9;
    return 0;
}

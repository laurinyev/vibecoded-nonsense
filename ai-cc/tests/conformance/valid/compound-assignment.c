struct Bits {
    unsigned int value : 4;
};

int main(void) {
    int values[3] = { 1, 0, 0 };
    int index = 0;
    int *pointer = values;
    signed char narrow = 126;
    struct Bits bits = { 1 };
    float fraction = 1.5f;
    int shifted = 2;

    if ((values[index++] += 3) != 4 || index != 1 || values[0] != 4) return 1;
    pointer += 2;
    if (pointer != values + 2) return 2;
    if ((narrow += 2) != -128) return 3;
    if ((bits.value += 5) != 6 || bits.value != 6) return 4;
    if ((fraction *= 2.0f) != 3.0f) return 5;
    shifted <<= 2;
    if (shifted != 8) return 6;
    return 0;
}

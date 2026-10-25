int main(void) {
    int value = 0;
    int *pointer = &value;
    const int *qualified = pointer;
    int *invalid = 1 ? pointer : qualified;
    return invalid != 0;
}

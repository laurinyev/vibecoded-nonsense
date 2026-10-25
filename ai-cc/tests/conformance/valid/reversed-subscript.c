int main(void) {
    int values[3] = { 4, 5, 6 };
    int *pointer = values;

    if (0[pointer] != 4) return 1;
    if (2[values] != 6) return 2;
    1[pointer] = 9;
    return values[1] != 9;
}

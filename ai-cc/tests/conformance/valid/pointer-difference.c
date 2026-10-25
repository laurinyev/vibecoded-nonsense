struct Pair {
    int first;
    int second;
};

int main(void) {
    int values[5];
    struct Pair pairs[3];
    int *first = &values[1];
    int *last = &values[4];

    if (last - first != 3) return 1;
    if (&pairs[2] - pairs != 2) return 2;
    return 0;
}

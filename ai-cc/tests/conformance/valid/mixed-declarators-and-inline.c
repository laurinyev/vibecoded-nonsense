static inline int helper(void) {
    return 3;
}

extern inline int external_helper(void) {
    return 4;
}

int second(void), object = 5, values[2] = { 6, 7 };

static int second(void) {
    return values[0] + values[1];
}

int main(void) {
    if (helper() != 3) return 1;
    if (external_helper() != 4) return 2;
    if (object != 5) return 3;
    if (second() != 13) return 4;
    return 0;
}

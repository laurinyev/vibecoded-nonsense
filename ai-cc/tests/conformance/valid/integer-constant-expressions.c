enum Values {
    BASE = 2,
    SHIFTED = 1 << BASE,
    LAZY = 1 ? SHIFTED + 1 : 1 / 0,
    SHORTED = 0 && 1 / 0
};

struct Flags {
    unsigned int enabled : (0 && 1 / 0) + 1;
};

int global_values[BASE + 2];
int sized_values[sizeof(int)];
int lazy_values[SHORTED ? 1 : 3];
int designated_values[4] = { [0 && 1 / 0] = 3, [BASE - 1] = 9 };
int global_value = 6;
int *global_pointer = &global_value;
int lazy_global = 1 ? 7 : 1 / 0;

int main(void) {
    int local_values[sizeof(int)];
    struct Flags flags = { 0 };
    static int static_value = 8;
    static int *saved_pointer = &global_value;
    static int *saved_static_pointer = &static_value;
    flags.enabled = 1;
    if (sizeof(global_values) != 16) return 1;
    if (sizeof(sized_values) != 16) return 2;
    if (sizeof(lazy_values) != 12) return 3;
    if (sizeof(local_values) != 16) return 4;
    if (designated_values[0] != 3 || designated_values[1] != 9 || flags.enabled != 1) return 5;
    if (*global_pointer != 6 || *saved_pointer != 6) return 6;
    if (*saved_static_pointer != 8) return 8;
    if (lazy_global != 7) return 7;
    return LAZY != 5 || SHIFTED != 4;
}

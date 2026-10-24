int inc(int x) { return x + 1; }
int dec(int x) { return x - 1; }

int (*global_fn)(int) = inc;
int (*global_ops[2])(int) = { inc, dec };

int apply(int (*fn)(int), int value) {
    return fn(value);
}

int (*choose(int which))(int) {
    if (which) return inc;
    return dec;
}

int main(void) {
    int (*p)(int) = inc;
    if (p(4) != 5) return 1;
    if (apply(p, 9) != 10) return 2;
    p = dec;
    if (p(4) != 3) return 3;
    if ((*p)(4) != 3) return 4;
    if (choose(1)(10) != 11) return 5;
    if (choose(0)(10) != 9) return 6;

    int (*ops[2])(int) = { inc, dec };
    if (ops[0](7) != 8) return 7;
    if (ops[1](7) != 6) return 8;

    if (global_fn(3) != 4) return 9;
    if (global_ops[0](3) != 4) return 10;
    if (global_ops[1](3) != 2) return 11;
    global_fn = dec;
    if (global_fn(3) != 2) return 12;

    struct table { int (*fn)(int); } table = { inc };
    if (table.fn(20) != 21) return 13;

    static int (*static_fn)(int) = inc;
    if (static_fn(30) != 31) return 14;

    int (*inc)(int) = dec;
    if (inc(5) != 4) return 15;

    return 0;
}

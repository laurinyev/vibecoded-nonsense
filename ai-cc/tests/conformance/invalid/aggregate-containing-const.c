struct S {
    int value;
    const int fixed;
};

int main(void) {
    struct S left, right;
    left = right;
    return 0;
}

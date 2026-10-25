struct S {
    int value;
};

int main(void) {
    struct S left = { 1 };
    struct S right = { 2 };
    left.value = 3;
    left = right;
    return left.value != 2;
}

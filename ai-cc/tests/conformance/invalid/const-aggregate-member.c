struct S {
    int value;
};

int main(void) {
    const struct S object = { 0 };
    object.value = 1;
    return 0;
}

struct S {
    int value;
};

int main(void) {
    const struct S *object;
    object->value = 1;
    return 0;
}

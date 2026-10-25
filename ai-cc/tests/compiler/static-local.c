int next_value(void) {
    static int value = 4;
    value++;
    return value;
}

int main(void) {
    return next_value() + next_value() - 11;
}

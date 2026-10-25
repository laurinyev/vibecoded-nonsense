int main(void) {
    int automatic;
    static int *invalid = &automatic;
    return invalid != 0;
}

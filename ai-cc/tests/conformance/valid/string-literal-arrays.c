int main(void) {
    char *text = "abc";
    char (*array_pointer)[4] = &"abc";

    if (sizeof "abc" != 4) return 1;
    if ("abc"[1] != 'b' || text[2] != 'c') return 2;
    if (sizeof *array_pointer != 4) return 3;
    return 0;
}

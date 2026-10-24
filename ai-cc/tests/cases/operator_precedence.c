int main() {
    int x = 1 + 2 * 3;
    x <<= 1;
    x |= 1;
    x ^= 3;
    x &= 7;
    x = +x;
    if (0) {
        x = ~x;
        x = x >> 1;
        x = x / 2 % 100;
    }
    x++;
    --x;
    x = x > 3 ? x : 0;
    x = (0, x + 67);
    if ((x == 71 && x != 0) || 0) putchar(x);
    putchar(10);
    return 0;
}

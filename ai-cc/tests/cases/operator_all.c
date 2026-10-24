int main() {
    int x = (1 + 2 * 3 == 7) ? 65 : 0;
    x += 1;
    x -= 1;
    x *= 1;
    x /= 1;
    x %= 100;
    x <<= 0;
    x >>= 0;
    x &= 127;
    x ^= 0;
    x |= 0;
    x = +x;
    x = ~x;
    x = ~x;
    x++;
    --x;
    x = (x < 100 && x <= 65 && x >= 65 && x > 0 && x != 0) ? x : 0;
    int side = 0;
    0 && (side = 1);
    1 || (side = 2);
    x += side;
    x = (0, x);
    putchar(x);
    putchar(10);
    return 0;
}

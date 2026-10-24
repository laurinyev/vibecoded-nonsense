/* EXPECT-DIAGNOSTIC: bit-field width exceeds its declared type */
struct Invalid { unsigned value : 33; };

int main(void) {
    return 0;
}

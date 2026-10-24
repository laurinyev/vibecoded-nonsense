/* EXPECT-DIAGNOSTIC: no member named missing */
struct Pair { int value; };

int main(void) {
    struct Pair pair;
    return pair.missing;
}

/* EXPECT-DIAGNOSTIC: cannot take the address of a bit-field */
struct Flags { unsigned value : 3; };

int main(void) {
    struct Flags flags = { 1 };
    return &flags.value != 0;
}

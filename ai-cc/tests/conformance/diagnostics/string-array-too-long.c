/* EXPECT-DIAGNOSTIC: initializer string is too long for array */
int main(void) {
    char value[2] = "abc";
    return value[0];
}

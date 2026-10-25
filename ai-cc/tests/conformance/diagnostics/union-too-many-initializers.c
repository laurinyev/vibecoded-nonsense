/* EXPECT-DIAGNOSTIC: too many union initializers */
union Value {
    int first;
    int second;
};

int main(void) {
    union Value value = { 1, 2 };
    return value.first;
}

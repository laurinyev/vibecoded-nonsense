/* EXPECT-DIAGNOSTIC: macro invocation has incorrect number of arguments */
#define ADD(left, right) ((left) + (right))

int main(void) {
    return ADD(1);
}

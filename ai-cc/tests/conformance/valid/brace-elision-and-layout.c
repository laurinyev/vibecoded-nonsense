struct Inner { char tag; int value; };
struct Outer { char lead; struct Inner inner; int tail; };
union Mixed { char c; long l; };
struct Padded { char a; int b; char c; };

static struct Outer global_outer = { 'A', { 'B', 7 }, 9 };
static int global_matrix[2][3] = { 1, 2, 3, 4, 5, 6 };

int main(void) {
    struct Outer local_outer = { 'C', { 'D', 11 }, 13 };
    struct Inner locals[2] = { { 'E', 17 }, { 'F', 19 } };
    int matrix[2][3] = { 1, 2, 3, 4, 5, 6 };
    union Mixed mixed = { 5 };
    int total = 0;

    total += sizeof(struct Padded) == 12;
    total += sizeof(union Mixed) == 8;
    total += sizeof(struct Outer) == 16;

    total += global_outer.lead == 'A';
    total += global_outer.inner.tag == 'B';
    total += global_outer.inner.value == 7;
    total += global_outer.tail == 9;

    total += local_outer.lead == 'C';
    total += local_outer.inner.tag == 'D';
    total += local_outer.inner.value == 11;
    total += local_outer.tail == 13;

    total += locals[0].tag == 'E';
    total += locals[0].value == 17;
    total += locals[1].tag == 'F';
    total += locals[1].value == 19;

    total += matrix[0][2] == 3;
    total += matrix[1][0] == 4;
    total += matrix[1][2] == 6;

    total += global_matrix[0][1] == 2;
    total += global_matrix[1][2] == 6;

    total += mixed.c == 5;

    return total - 21;
}

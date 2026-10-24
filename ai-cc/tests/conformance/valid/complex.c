static _Complex double global_complex = 1.0 + 2.0i;
static _Complex float global_complex_float = 1.5f + 2.5fi;

struct Box {
    _Complex double value;
    int tag;
};

static struct Box global_box = { .tag = 7, .value = 3.0 - 4.0i };

_Complex double add(_Complex double a, _Complex double b) {
    return a + b;
}

_Complex double multiply(_Complex double a, _Complex double b) {
    return a * b;
}

_Complex double divide(_Complex double a, _Complex double b) {
    return a / b;
}

_Complex float narrow(_Complex double value) {
    return value;
}

double real_part(_Complex double value) {
    return (double)value;
}

int main(void) {
    int total = 0;

    total += sizeof(_Complex float) == 8;
    total += sizeof(_Complex double) == 16;

    total += global_complex == 1.0 + 2.0i;
    total += global_complex_float == 1.5f + 2.5fi;
    total += global_box.value == 3.0 - 4.0i;
    total += global_box.tag == 7;

    _Complex double a = 1.0 + 2.0i;
    _Complex double b = 3.0 - 1.0i;

    total += a != b;
    total += a == 1.0 + 2.0i;
    total += real_part(a) == 1.0;
    total += (double)a == 1.0;

    _Complex double sum = add(a, b);
    total += sum == 4.0 + 1.0i;
    total += a + b == sum;
    total += a - b == -2.0 + 3.0i;

    _Complex double product = multiply(a, b);
    total += product == 5.0 + 5.0i;
    total += a * b == product;

    _Complex double quotient = divide(product, b);
    total += quotient == a;
    total += product / b == a;

    total += -a == -1.0 - 2.0i;
    total += +a == a;
    total += !a == 0;
    total += !(_Complex double)0.0 == 1;
    total += !!(a != 0.0) == 1;

    _Complex double zero = 0.0;
    total += zero == 0.0;
    total += (zero ? 1 : 0) == 0;
    total += (a ? 1 : 0) == 1;

    _Complex float small = narrow(a);
    total += small == 1.0f + 2.0fi;
    total += (double)small == 1.0;
    total += (_Complex double)small == a;

    _Complex double from_real = 2;
    total += from_real == 2.0 + 0.0i;
    total += (_Complex double)3.5f == 3.5 + 0.0i;

    _Complex double picked = (b != 0.0 ? a : b);
    total += picked == a;

    static _Complex double local = 4.0 + 5.0i;
    total += local == 4.0 + 5.0i;

    _Complex double array[2] = { 1.0 + 1.0i, 2.0 + 2.0i };
    total += array[0] == 1.0 + 1.0i;
    total += array[1] == 2.0 + 2.0i;

    return total - 34;
}

static double global_double = 1.25 + 0.75;
static float global_float = 0x1.8p1f;
static long double global_long_double = 2.5L;
static int global_converted = (int)9.75;

struct Pair {
    float low;
    double high;
};

static struct Pair global_pair = { .high = 2.5, .low = 1.5 };

float scale(float value) {
    return value * 3.0f;
}

double midpoint(double a, double b) {
    return (a + b) / 2.0;
}

int main(void) {
    int total = 0;

    total += sizeof(float) == 4;
    total += sizeof(double) == 8;
    total += sizeof(long double) == 16;

    total += global_double == 2.0;
    total += global_float == 3.0f;
    total += global_long_double == 2.5L;
    total += global_converted == 9;
    total += global_pair.low == 1.5f;
    total += global_pair.high == 2.5;

    static float static_float = 4.5f;
    static double static_double = -1.25;
    total += static_float == 4.5f;
    total += static_double == -1.25;

    float f = 1.25f;
    double d = 3.5;
    long double ld = 6.0L;

    total += f + 0.75f == 2.0f;
    total += d - 0.5 == 3.0;
    total += f * 4.0f == 5.0f;
    total += d / 2.0 == 1.75;
    total += ld / 3.0L == 2.0L;

    total += -f == -1.25f;
    total += +d == 3.5;
    total += !0.0 == 1;
    total += !0.25 == 0;

    total += f < 2.0f;
    total += d > 3.0;
    total += d <= 3.5;
    total += d >= 3.5;
    total += d != 4.0;
    total += ld == 6.0L;

    total += (int)d == 3;
    total += (int)-1.75 == -1;
    total += (float)7 == 7.0f;
    total += (double)f == 1.25;
    total += (long double)d == 3.5L;
    total += (float)d == 3.5f;
    total += (double)(float)1.1 == (double)1.1f;

    total += 1 + 0.5 == 1.5;
    total += 2.0f + 3 == 5.0f;
    total += f < 2;

    total += scale(2.0f) == 6.0f;
    total += midpoint(1.0f, 4.0) == 2.5;

    total += (d > 0.0 ? f : 9.0f) == 1.25f;
    total += (d < 0.0 ? f : 9.0f) == 9.0f;

    double hex = 0x1.8p+2;
    float suffix = 2.0F;
    double exponent = 1.5e2;
    total += hex == 6.0;
    total += suffix == 2.0f;
    total += exponent == 150.0;

    return total - 43;
}

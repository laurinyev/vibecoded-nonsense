struct Flags { unsigned a : 1; unsigned b : 2; unsigned c : 5; };
struct Mixed { char lead; unsigned x : 4; int y : 9; char tail; };
struct Small { unsigned char a : 3; unsigned short b : 6; unsigned char c : 2; };
struct Split { unsigned a : 3; unsigned : 0; unsigned b : 5; };
struct Packed { unsigned a : 3, b : 5; };
union Choice { unsigned bits : 5; char byte; };

static struct Flags global_flags = { 1, 3, 17 };
static struct Mixed global_mixed = { 'g', 9, -17, 'h' };
static struct Mixed global_designated = {
    .tail = 't', .y = 42, .lead = 'l', .x = 3,
};
static struct Small global_small = { 5, 33, 2 };
static struct Split global_split = { 5, 9 };
static struct Packed global_packed = { 7, 31 };

int main(void) {
    struct Flags f = { 1, 2, 5 };
    struct Mixed m = { 'a', 7, -33, 'b' };
    struct Mixed d = { .tail = 'z', .x = 4, .lead = 'q', .y = -1 };
    struct Small sm = { 3, 17, 1 };
    struct Split sp = { 6, 12 };
    struct Packed packed = { 2, 30 };
    union Choice u = { .bits = 21 };
    struct Mixed *p = &m;
    int total = 0;

    total += sizeof(struct Flags) == 4;
    total += sizeof(struct Mixed) == 4;
    total += sizeof(struct Small) == 2;
    total += sizeof(struct Split) == 8;
    total += sizeof(union Choice) == 4;
    total += sizeof(struct Packed) == 4;

    total += global_flags.a == 1;
    total += global_flags.b == 3;
    total += global_flags.c == 17;

    total += global_mixed.lead == 'g';
    total += global_mixed.x == 9;
    total += global_mixed.y == -17;
    total += global_mixed.tail == 'h';

    total += global_designated.lead == 'l';
    total += global_designated.x == 3;
    total += global_designated.y == 42;
    total += global_designated.tail == 't';

    total += global_small.a == 5;
    total += global_small.b == 33;
    total += global_small.c == 2;

    total += global_split.a == 5;
    total += global_split.b == 9;

    total += global_packed.a == 7;
    total += global_packed.b == 31;

    total += f.a == 1;
    total += f.b == 2;
    total += f.c == 5;

    total += m.lead == 'a';
    total += m.x == 7;
    total += m.y == -33;
    total += m.tail == 'b';

    total += d.lead == 'q';
    total += d.x == 4;
    total += d.y == -1;
    total += d.tail == 'z';

    total += sm.a == 3;
    total += sm.b == 17;
    total += sm.c == 1;

    total += sp.a == 6;
    total += sp.b == 12;
    total += packed.a == 2;
    total += packed.b == 30;
    total += u.bits == 21;
    total += p->y == -33;

    f.a = 0;
    f.b = 3;
    f.c = 31;
    total += f.a == 0;
    total += f.b == 3;
    total += f.c == 31;

    p->x = 15;
    p->y = -5;
    p->tail = 'c';
    total += m.x == 15;
    total += m.y == -5;
    total += m.tail == 'c';
    total += p->lead == 'a';

    d.x = 0;
    d.y = -256;
    total += d.x == 0;
    total += d.y == -256;

    sm.a = 7;
    sm.b = 63;
    sm.c = 3;
    total += sm.a == 7;
    total += sm.b == 63;
    total += sm.c == 3;

    sp.a = 7;
    sp.b = 31;
    total += sp.a == 7;
    total += sp.b == 31;

    u.bits = 30;
    total += u.bits == 30;

    return total - 59;
}

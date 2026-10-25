#define VALUE 4
#define ADD(a, b) ((a) + (b))
#define STR(x) #x
#define CAT(a, b) a ## b
#define JOIN(first, ...) first __VA_ARGS__
#define LEFT RIGHT
#define RIGHT LEFT
ADD(VALUE, 3)
STR(VALUE)
CAT(ma, in)
JOIN(1, 2, 3)
LEFT
#if defined(VALUE) && (2 + 3 * 4 == 14)
selected
#else
not_selected
#endif
#undef VALUE
#ifdef VALUE
wrong
#else
undefined
#endif

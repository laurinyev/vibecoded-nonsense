int main(void) { int a = 2, b = a + 3, *p, (*row)[3]; const int c = 4; int const d = 5; int * const cp = &b; p = &b; return *p + c + d; }

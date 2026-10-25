struct S { int a; }; int S = 3; int main(void) { struct S value; value.a = S; return value.a - 3; }

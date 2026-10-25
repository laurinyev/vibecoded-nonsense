struct S { int a; }; int main(void) { struct S outer; { struct S { char b; }; struct S inner; inner.b = 1; } outer.a = 2; return outer.a - 2; }

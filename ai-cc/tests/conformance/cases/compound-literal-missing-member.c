struct S { int value; }; int main(void) { return (struct S){ .missing = 1 }.value; }

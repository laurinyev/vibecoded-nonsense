union U { int value; char byte; }; int main(void) { return (union U){ 1, 2 }.value; }

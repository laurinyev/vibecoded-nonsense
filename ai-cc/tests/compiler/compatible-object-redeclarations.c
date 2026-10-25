extern int shared[];
extern int shared[3];
int shared[3] = {1, 2, 3};
int completed[3];
int completed[] = {1, 2};
int main(void) { return shared[1] - 2; }

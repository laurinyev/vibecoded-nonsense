extern int external_tentative;
int external_tentative;
int initialized = 7;
static int internal_tentative;
static const unsigned long constant_value = 9;
int *global_pointer = 0;
int (*callback)(int) = 0;

int main(void) {
    external_tentative = initialized + (int)constant_value;
    internal_tentative = external_tentative;
    global_pointer = &initialized;
    return *global_pointer + internal_tentative - 23;
}

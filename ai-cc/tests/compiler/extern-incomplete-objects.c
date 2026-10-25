struct opaque;
extern struct opaque external;
extern int external_array[];
int main(void) {
    extern struct opaque local;
    extern int local_array[];
    return 0;
}

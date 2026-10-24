static char global[] = "hello";
static char padded[8] = "hi";
static char exact[3] = "abc";

int main(void) {
    char local[] = "world";
    char local_padded[6] = "ab";
    int total = 0;

    total += sizeof global;
    total += sizeof local;
    total += sizeof padded;
    total += sizeof exact;
    total += global[0] == 'h';
    total += local[4] == 'd';
    total += padded[2] == 0;
    total += padded[7] == 0;
    total += exact[2] == 'c';
    total += local_padded[2] == 0;
    total += local_padded[5] == 0;

    local[0] = 'W';
    total += local[0] == 'W';

    return total - 31;
}

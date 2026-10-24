typedef int Number;

int main(void) {
    Number outer = 1;
    {
        typedef double Number;
        Number inner = 2.5;
        if (inner != 2.5) return 1;
        if (sizeof(Number) != sizeof(double)) return 2;
        {
            typedef char Number;
            Number tiny = 3;
            if (tiny != 3) return 3;
            if (sizeof(Number) != 1) return 4;
        }
        if (sizeof(Number) != sizeof(double)) return 5;
    }
    if (sizeof(Number) != sizeof(int)) return 6;
    Number final = 4;
    return outer + final - 5;
}

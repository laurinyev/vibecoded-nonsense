typedef unsigned char SmallStorage[5];
typedef unsigned char ThreeBytes[3];
typedef unsigned char SixBytes[6];
typedef unsigned char SevenBytes[7];

int main(void) {
    SmallStorage source = { 7, 11, 13, 17, 19 };
    SmallStorage destination = { 0 };
    ThreeBytes three_source = { 2, 3, 5 };
    ThreeBytes three_destination = { 0 };
    SixBytes six_source = { 1, 2, 3, 4, 5, 6 };
    SixBytes six_destination = { 0 };
    SevenBytes seven_source = { 1, 2, 3, 4, 5, 6, 7 };
    SevenBytes seven_destination = { 0 };

    va_copy(destination, source);
    va_copy(three_destination, three_source);
    va_copy(six_destination, six_source);
    va_copy(seven_destination, seven_source);
    if (destination[0] != 7 || destination[1] != 11) return 1;
    if (destination[2] != 13 || destination[3] != 17 || destination[4] != 19) return 2;
    if (three_destination[0] != 2 || three_destination[1] != 3 || three_destination[2] != 5) return 3;
    if (six_destination[0] != 1 || six_destination[5] != 6) return 4;
    if (seven_destination[0] != 1 || seven_destination[6] != 7) return 5;
    return 0;
}

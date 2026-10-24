int sum_four(int values[const static 4]) {
    return values[0] + values[1] + values[2] + values[3];
}

int main(void) {
    int values[4] = { 1, 2, 3, 4 };
    return sum_four(values) - 10;
}

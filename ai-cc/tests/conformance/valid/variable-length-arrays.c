int sum_row(int count, int row[count]) {
    int total = 0;
    for (int i = 0; i < count; i++) {
        total += row[i];
    }
    return total;
}

int main(void) {
    int n = 4;
    int values[n];
    for (int i = 0; i < n; i++) {
        values[i] = i * 3;
    }
    if (sum_row(n, values) != 18) return 1;
    if (sizeof(values) != n * sizeof(int)) return 2;

    int rows = n - 2;
    int matrix[rows][3];
    for (int i = 0; i < rows; i++) {
        for (int j = 0; j < 3; j++) {
            matrix[i][j] = i + j;
        }
    }
    if (matrix[0][0] != 0) return 3;
    if (matrix[1][2] != 3) return 4;
    if (sizeof(matrix) != rows * 3 * sizeof(int)) return 5;

    int counter = 1;
    int expanded[counter + 2];
    for (int i = 0; i < counter + 2; i++) {
        expanded[i] = i;
    }
    counter++;
    if (expanded[2] != 2) return 6;
    return 0;
}

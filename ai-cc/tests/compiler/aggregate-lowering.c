typedef struct {
    int score;
    char cells[4];
} Board;

static void update(Board *board, unsigned int seed) {
    board->score = seed;
    board->cells[2] = 'X';
}

int main(void) {
    Board board;
    int values[3];
    int *cursor;

    values[0] = 4;
    values[1] = 5;
    values[2] = 6;
    cursor = &values[1];
    *cursor = values[0] + values[2];
    board.score = 0;
    update(&board, values[1]);
    return board.score == 10 && board.cells[2] == 'X' ? 0 : 1;
}

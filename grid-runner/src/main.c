#include "game.h"

int main() {
    Game game;
    int moves[12][2];
    int index;
    int result;

    moves[0][0] = 1; moves[0][1] = 0;
    moves[1][0] = 1; moves[1][1] = 0;
    moves[2][0] = 0; moves[2][1] = 1;
    moves[3][0] = 1; moves[3][1] = 0;
    moves[4][0] = 0; moves[4][1] = 1;
    moves[5][0] = 1; moves[5][1] = 0;
    moves[6][0] = 1; moves[6][1] = 0;
    moves[7][0] = 0; moves[7][1] = 1;
    moves[8][0] = 1; moves[8][1] = 0;
    moves[9][0] = 1; moves[9][1] = 0;
    moves[10][0] = 0; moves[10][1] = 1;
    moves[11][0] = 1; moves[11][1] = 0;

    game_init(&game, 73);
    game_render(&game);
    for (index = 0; index < 12; index = index + 1) {
        result = game_step(&game, moves[index][0], moves[index][1]);
        game_render(&game);
        if (result != 0) {
            break;
        }
    }
    return game.score < 0;
}

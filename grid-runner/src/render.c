#include "game.h"
#include <stdio.h>

void game_render(const Game *game) {
    int x;
    int y;

    printf("turn %d energy %d score %d loot %d hazards %d\n",
           game->turn, game->energy, game->score,
           game->collected, game->hazards);
    for (y = 0; y < game->height; y = y + 1) {
        for (x = 0; x < game->width; x = x + 1) {
            putchar(game->tiles[y][x]);
        }
        putchar('\n');
    }
    putchar('\n');
}

#include "game.h"

void world_generate(Game *game) {
    int x;
    int y;
    unsigned int roll;

    game->exit.x = game->width - 2;
    game->exit.y = game->height - 2;
    for (y = 0; y < game->height; y = y + 1) {
        for (x = 0; x < game->width; x = x + 1) {
            if (x == 0 || y == 0 || x == game->width - 1 || y == game->height - 1) {
                game->tiles[y][x] = '#';
            } else {
                roll = rng_next(&game->seed) % 10;
                if (roll < 2 && !(x == 1 && y == 1)) {
                    game->tiles[y][x] = '#';
                } else if (roll == 2) {
                    game->tiles[y][x] = '*';
                } else if (roll == 3) {
                    game->tiles[y][x] = '!';
                } else {
                    game->tiles[y][x] = '.';
                }
            }
        }
    }
    game->tiles[game->exit.y][game->exit.x] = 'X';
    game->tiles[1][1] = '.';
}

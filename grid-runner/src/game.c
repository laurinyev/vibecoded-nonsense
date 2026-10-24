#include "game.h"

static void place_player(Game *game) {
    game->player.x = 1;
    game->player.y = 1;
    game->tiles[game->player.y][game->player.x] = 'P';
}

void game_init(Game *game, unsigned int seed) {
    game->width = GRID_WIDTH;
    game->height = GRID_HEIGHT;
    game->energy = 20;
    game->score = 0;
    game->turn = 0;
    game->collected = 0;
    game->hazards = 0;
    game->seed = seed;
    world_generate(game);
    place_player(game);
}

static int valid_position(const Game *game, int x, int y) {
    if (x < 0 || y < 0 || x >= game->width || y >= game->height) {
        return 0;
    }
    return game->tiles[y][x] != '#';
}

int game_step(Game *game, int dx, int dy) {
    int next_x = game->player.x + dx;
    int next_y = game->player.y + dy;
    char tile;

    if (!valid_position(game, next_x, next_y)) {
        game->energy = game->energy - 1;
        game->turn = game->turn + 1;
        return game->energy <= 0;
    }
    game->tiles[game->player.y][game->player.x] = '.';
    game->player.x = next_x;
    game->player.y = next_y;
    tile = game->tiles[next_y][next_x];
    if (tile == '*') {
        game->energy = game->energy + 4;
        game->score = game->score + 10;
        game->collected = game->collected + 1;
    } else if (tile == '!') {
        game->energy = game->energy - 5;
        game->score = game->score - 3;
        game->hazards = game->hazards + 1;
    } else {
        game->score = game->score + 1;
    }
    game->tiles[next_y][next_x] = 'P';
    game->energy = game->energy - 1;
    game->turn = game->turn + 1;
    if (game->player.x == game->exit.x && game->player.y == game->exit.y) {
        game->score = game->score + game->energy;
        return 1;
    }
    return game->energy <= 0;
}

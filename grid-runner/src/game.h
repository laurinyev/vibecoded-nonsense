#ifndef GRID_RUNNER_GAME_H
#define GRID_RUNNER_GAME_H

#define GRID_WIDTH 16
#define GRID_HEIGHT 8

typedef struct {
    int x;
    int y;
} Position;

typedef struct {
    int width;
    int height;
    char tiles[GRID_HEIGHT][GRID_WIDTH];
    Position player;
    Position exit;
    int energy;
    int score;
    int turn;
    int collected;
    int hazards;
    unsigned int seed;
} Game;
void game_init(Game *game, unsigned int seed);
int game_step(Game *game, int dx, int dy);
void game_render(const Game *game);
void world_generate(Game *game);
unsigned int rng_next(unsigned int *seed);

#endif

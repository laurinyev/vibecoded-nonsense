struct Pair { int left; int right; };
union Number { int word; char byte; };

static int *global_sparse = (int[]){ [2] = 11, [0] = 5 };
static struct Pair global_pair = (struct Pair){ .right = 9, .left = 7 };

int main(void) {
    int *sparse = (int[]){ [2] = 9, [0] = 4 };
    struct Pair *pair = &(struct Pair){ .right = 8, .left = 3 };
    int (*matrix)[2] = (int[][2]){ [1][0] = 3, [1][1] = 4, [0][0] = 1, [0][1] = 2 };
    union Number number = (union Number){ .word = 10 };

    sparse[1] = 6;
    pair->left = pair->left + 2;
    return sparse[0] + sparse[1] + sparse[2]
        + pair->left + pair->right + matrix[1][0] + matrix[1][1]
        + global_sparse[0] + global_sparse[1] + global_sparse[2]
        + global_pair.left + global_pair.right + number.word - 81;
}

struct Pair { int left; int right; };

int main(void) {
    struct Pair p;
    p.left = 4;
    p.right = 5;
    return p.left + p.right;
}

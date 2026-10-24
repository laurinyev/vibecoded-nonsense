int (identity)(int value) {
    return value;
}

int (*factory(int seed))(int) {
    (void)seed;
}

int (*old_factory(seed))(int)
int seed;
{
    (void)seed;
}

int main(void) {
    return identity(12) - 12;
}

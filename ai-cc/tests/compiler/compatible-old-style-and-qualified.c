int value;
signed int value;
extern const int constant_value;
extern const int constant_value;
int helper();
int helper(int argument);
int helper(int argument) { return argument; }
int main(void) { return helper(0); }

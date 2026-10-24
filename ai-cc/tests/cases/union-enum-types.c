union Payload { int number; const char marker; }; enum State { Idle, Busy=7, Done }; int main(){ union Payload p; enum State s; volatile int clock; return 0; }

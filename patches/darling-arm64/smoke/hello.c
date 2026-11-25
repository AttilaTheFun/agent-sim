// Phase 1 smoke test: an iOS-simulator arm64 program, run through the runtime's own dyld_sim.
// Linked against a stub libSystem.B.dylib (our own), so dyld_sim binds puts/getpid from the runtime's shared cache.
int puts(const char *);
int printf(const char *, ...);
int getpid(void);
int main(int argc, char **argv) {
    puts("hello from an iOS simulator arm64 program");
    printf("argc=%d pid=%d\n", argc, getpid());
    return 42;
}

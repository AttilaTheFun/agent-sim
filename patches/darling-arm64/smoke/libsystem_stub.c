// Stub exporting the symbols hello.c imports; never loaded at run time (dyld_sim uses the runtime's libSystem).
int puts(const char *s) { return 0; }
int printf(const char *f, ...) { return 0; }
int getpid(void) { return 0; }
void dyld_stub_binder(void) __asm__("dyld_stub_binder");
void dyld_stub_binder(void) {}

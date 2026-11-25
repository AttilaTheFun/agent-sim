# Darling for the iOS simulator on arm64 Linux

Patches that turn darlinghq/darling `master` (the commits in `BASE.txt`; arm64 host support is darling#1818) into a
Darling that runs Apple's arm64 iOS simulator runtime natively on arm64 Linux (docs/ARM64.md).
`scripts/setup-arm64.sh` checks out those commits and applies them. They change Darling's code and carry x86Sim's,
so they are under those projects' licenses (Darling's GPL-3.0 and its components' own; x86Sim's forks are
GPL-3.0-or-later), not agent-sim's Apache-2.0.

Each `<name>.patch` is the whole difference from the commit master pins for that submodule (`darling.patch`: the
top-level repository) to the tree that was built and booted. Most of it is x86Sim's work: the `x86sim` branches of
its Darling forks (the commits in `BASE.txt`), merged into master's commits. The rest is the merge and the arm64 port.

## x86Sim's Darling changes, merged

x86Sim's forks are 5-129 commits ahead of upstream (xnu 129, darlingserver 73, libkqueue 16, iokitd 9, libplatform
6, libpthread 5, dyld 5, libunwind 4, sqlite 4; docs/REPOS.md in x86Sim lists them): workloops and the cooperative
thread pool, Mach memory entries and VM maps, exceptions and host ports, file flags, PF_ROUTE, csops, IOSurface
memfds, and the iOS-simulator exports. Twelve files conflicted:

- **xnu:** where x86Sim and this port fixed the same thing (Darling's `_libkernel_functions` additions are only
  valid in its own version 4 structure; `F_ADDFILESIGS_FOR_DYLD_SIM`), x86Sim's version, plus the guard it lacked on
  master's arm64 TSD lookup. `sigexc.c` and `mach_traps.c`: both sides.
- **darlingserver:** master's thread-suspension design (`suspension_gate`) over x86Sim's (`block_suspend_gate`):
  both fix the same race and master's is newer; x86Sim's removal of dead threads from their task's list kept.
  `processSignal` keeps master's exception state.
- **libkqueue:** both new filters (master's EVFILT_MEMORYSTATUS, x86Sim's EVFILT_SOCK); x86Sim's per-knote
  EV_UDATA_SPECIFIC.
- cocotron, metal and swift (macOS-side, unused by the simulator) stay at master's commits.

## The arm64 port

| Where | What and why |
|---|---|
| darlingserver `process.cpp` | Every process died at once on kernels without `CONFIG_CROSS_MEMORY_ATTACH` (Apple's containerization kernel): `process_vm_readv`/`writev` answer `ENOSYS`. Falls back to `/proc/<pid>/mem`. |
| darlingserver `memory.c` | Client memory was rounded to duct-tape's 16K arm64 `PAGE_SIZE` (its VMAPPLE configuration) while Linux maps it in 4K pages: a range began up to 12K below its mapping. `mach_make_memory_entry` of libdispatch's firehose buffer failed (`KERN_INVALID_ADDRESS`, every process aborted at its first `os_log`) and syscall 599 handed mtlvk the wrong pages. Rounds to the host's page size (the same as before on x86-64). Also master's `#ifndef copyinmsg` guards, lost in the merge. |
| darlingserver `memory.c` (`mach_vm_region`) | Answered `KERN_INVALID_ADDRESS` for any region at or above x86-64's user limit (meant to hide `[vsyscall]`). arm64 Linux's user space reaches 2^48 and it maps hint-less memory near the top, out-of-line memory received in Mach messages among it, so backboardd's render server, which checks every buffer a client sends that way, refused them all: only IOSurface content (the wallpaper) was drawn. The limit is 2^48 on arm64. |
| darlingserver `fastcontext.cpp`, `thread.cpp`, `task.c` | x86Sim's x86-64-only context switch and debug-state flavors: built on x86-64 only (glibc's `getcontext`/`setcontext`, no task debug state elsewhere). |
| xnu emulation | x86Sim's syscalls that arm64 Linux lacks (`open`, `stat`, `readlink`, `poll`): their `*at`/`ppoll` forms (on both architectures). `struct stat64` -> `darling_stat64_t`. `__NR_openat2` defined. The workqueue's stack switch and stack-pointer read in arm64 assembly. `__builtin_ia32_pause` -> `yield`. `access(NULL)` answers `EFAULT` (os_variant asks it). |
| dyld (Darling's) | The SyscallHelpers table for `dyld_sim` at version 18. `dyld_sim`'s `TPIDRRO_EL0` reads become the TSD trap libsystem_kernel emulates. The simulator cache's (libdyld's `__tlv_get_addr`: every thread-local variable of every process) are rewritten when `dyld_sim` maps it, as dyld3 does for a macOS cache: a branch to a veneer that loads the TSD base from the native TLS slot. System counter reads (`mrs` of CNTVCT/CNTPCT and their self-synchronized forms: JavaScriptCore and VFX in the cache, and any in `dyld_sim`, the images it maps and the main executable, simulator images only) become a second trap, which answers Darling's `mach_absolute_time` (nanoseconds, where Apple silicon counts 24 MHz ticks). The cache's sites are kept in `<cache>.darling-sysregs`, written by the first process. |
| xnu emulation `openat.c`, `fsctl.c` | x86Sim's code read `st_mode` at x86-64's `struct stat` offset (24); on arm64 it is at 16, and 24 is `st_uid`. A directory opened with `O_SYMLINK` (and so `O_PATH` on Linux) was never reopened as a readable descriptor, so `getattrlistbulk` on it failed with `EBADF` and installd could not install an app (`getattrlistbulk failed on entry 0`). Reads the arm64 offset on arm64. |
| dyld (Darling's), environment | x86Sim's tools pass a simulator process's `DYLD_*` a second time as `X86SIM_A64_GENV_DYLD_*` for its translator, which hands them to the guest as they are (launchd_sim replaces an app's own `DYLD_INSERT_LIBRARIES` with its job's). Natively nothing did, so x86Sim's XCTest runner never got `libXCTestBundleInject.dylib` into a hosted test's app. Darling's dyld applies them before `dyld_sim` reads the environment. |
| libpthread | The main thread's structure belongs to the dyld that started the process; `dyld_sim`'s left its `thread_id` 0, so error-checking mutexes refused the main thread (`EDEADLK`; backboardd aborted in IOHIDEventSystem). Sets it. |
| mldr (`darling.patch`) | XNU's per-exec `apple[]` strings Darling left as TODOs: `stack_guard`, `malloc_entropy`, `executable_cdhash`, `executable_boothash` (libmalloc's xzone allocator aborts without the last). |
| libnetwork | Did not link on arm64 (nor did CFNetwork or Ruby's bundles): AArch64 GlobalMerge folded the static default contexts its asm aliases name. |

## Smoke test

`smoke/` holds a small iOS-simulator program built with Darling's own ld64 against a stub `libSystem.B.dylib` (no
Apple SDK), and its build script.

# macOS apps under Darling (plan, 2026-11-26)

The iOS simulator runs on arm64 Linux because Apple's simulator runtime brings its own userland (UIKit, backboardd) as
plain arm64 code and leaves the kernel interface to its host: Darling supplies that host (`libsystem_kernel`,
`libsystem_platform`, `libsystem_pthread`, and dyld's helpers for `dyld_sim`). A macOS app needs macOS's own userland,
AppKit, SkyLight and WindowServer included, which comes only from a macOS restore image, all of it arm64e and calling
the kernel itself. Logan (2026-11-26): this is like the SDKs rules_applecross already uses, not a macOS VM; the
`ios` platform first, then these milestones, each a go or no-go reported to Manager. macOS UI tests stay on macOS VMs
until D3 works.

## Milestones

- **D1: a command-line tool from macOS's own shared cache** (`/usr/bin/true`, then `sw_vers`), under Darling, in an
  arm64 Linux container.
- **D2: headless macOS XCTest** (logic tests of Foundation/Swift code), with XCTest from Xcode's MacOSX platform,
  through the same `bazel test` runner as iOS.
- **D3: WindowServer** under Darling, with framebuffer, HID and Metal stand-ins like the iOS simulator's (simfb, simhid,
  mtlvk). The largest risk: no one has done this in public.
- **D4: AppKit and SwiftUI windows draw**; screenshots cropped to the window.
- **D5: macOS XCUITest** (testmanagerd, accessibility, synthesized events).
- **D6: a Nimbus `macos-container` platform.**

## D1: what is known

Checked in a Nimbus container lease (Linux 6.18 on Apple's virtualization, arm64):

1. **Pointer authentication is off in the kernel** (`CONFIG_ARM64_PTR_AUTH` not set), and the CPU executes arm64e's
   instructions with the keys disabled: `pacia`/`autia` leave a pointer as it is, `blraa` and `ldraa` branch and load
   without authenticating, nothing traps. arm64e code runs as if it were arm64, and pointers dyld signs stay
   unsigned, consistently.
2. **macOS 27.2.1's arm64e shared cache** (26B131, 4115 images, 81 subcaches, 6.8 GB) comes out of Apple's public
   universal restore image with `ipsw extract --dyld --dyld-arch arm64e --device Mac14,8 --remote <ipsw url>` in a few
   minutes (it reads the system cryptex's APFS image through `apfs-fuse`, built from source; `/dev/fuse` is in the
   container). Like the iOS runtime, it is fetched from Apple onto the machine and never committed or published.
3. **The cache calls the kernel itself.** Unlike the simulator runtime, it holds `libsystem_kernel`,
   `libsystem_platform`, `libsystem_pthread` and dyld, whose system calls are `svc #0x80` with the number in x16. On
   Linux an `svc` from there would run whatever Linux system call x8 happens to name. Linux's syscall user dispatch,
   made for this, is x86-only here (`PR_SET_SYSCALL_USER_DISPATCH` answers EINVAL), but **a seccomp filter on the
   instruction pointer works**: calls from the cache's address range (0x180000000 up) trap to SIGSYS, whose handler
   gets x16 and the arguments, answers in x0 and the carry flag, and returns after the `svc`; calls from everywhere
   else run as usual. Tested with a raw Darwin `getpid` from that range.

## D1: what is left

- Start Apple's dyld from the cache (its `_dyld_start`) under Darling's loader (mldr), with the cache mapped at its
  unslid address so the seccomp filter's range is fixed for every process.
- The SIGSYS handler: Darwin's BSD system calls and Mach traps (negative x16) dispatched to Darling's existing
  emulation (libsystem_kernel's `sys_*` functions and darlingserver).
- The rewrites the iOS simulator's cache already gets: `TPIDRRO_EL0` reads (the thread's TSD) and system counter
  reads, in the macOS cache's executable mappings (patches/darling-arm64, dyld).
- macOS's own commpage layout and the shared-region calls dyld makes to map its cache (`shared_region_check_np`,
  `shared_region_map_and_slide_2_np`), answered from the cache Darling already mapped.

Estimate: a week or two to `true` and `sw_vers`. The go or no-go comes when a tool runs, or when one of the above
proves impossible.

# arm64 Linux, natively

agent-sim's phones run on x86-64 Linux, where x86Sim translates the runtime's arm64 code. On an arm64 Linux machine
none of that translation is needed: Apple's arm64 simulator runtime can run as it is, under Darling. This page is the
state of that port. It is work in progress, not yet wired into the CLI.

It picks up where [rules_applecross](https://github.com/apple-cross-toolchain/rules_applecross) leaves off: that
builds clean-room apps and test bundles on Linux, this runs them on Linux in a simulator that is not clean-room. A
project that wants only clean-room tools runs the same bundles in Apple's simulator on a macOS VM instead.

Where it goes from here, up to unit and UI tests run by `bazel test` in containers:
[ARM64-ROADMAP.md](ARM64-ROADMAP.md).

## What works

Measured in a Nimbus container (Apple's containerization kernel 6.18, arm64, 4K pages, 48-bit VA, 12 CPUs, 16-32 GB) on
the iOS 27.2 runtime (24B8117), with `scripts/setup-arm64.sh` and `scripts/boot-arm64.sh`:

- **dyld_sim and the shared cache.** Darling's dyld hands over to the runtime's `dyld_sim` as a Mac's does; the
  runtime's libraries come from its cache, Darling's arm64 `libsystem_kernel`, `libsystem_platform` and
  `libsystem_pthread` are the host libraries. A small iOS-simulator program starts in about 60 ms.
- **launchd_sim and the system.** simhost (x86Sim's, built for arm64) plays CoreSimulator's part of the handshake;
  `launchd_sim` brings up the device's daemons (x86Sim's standard job list: 107 jobs, among them backboardd,
  SpringBoard and PosterBoard). They stay up; geod and assistantd crashed once in a boot.
- **SpringBoard.** It finishes starting (it drops its `FBSystemShellStartup` assertion about two minutes into a
  first boot) and sits at the home screen: its accessibility tree (x86Sim's simax) shows the page indicator, the
  Search pill and the status bar clock.
- **Metal.** x86Sim's Metal stand-in runs unchanged: metalhostd (prebuilt, in the simulator) and mtlvk (built for
  arm64, on Mesa's lavapipe). x86Sim's `mtlprobe` passes 48 of its 49 checks; the one it fails checks a 1206-wide
  surface against a 64x64 triangle, so it fails whatever the GPU does.
- **The screen.** backboardd composites through Metal into simfb's frame ring. The home screen is drawn whole
  (wallpaper, status bar, icons with their labels, the Search pill, the dock) and so are apps: Settings shows its
  title, account card and list. Until this was fixed only IOSurface content (the wallpaper) reached the screen:
  the render server checks each buffer a client sends with `mach_vm_region`, and Darling's answered "no such
  address" above x86-64's user limit, where arm64 Linux maps received out-of-line memory.
- **Apps.** `cli/libexec/simctl.py listapps` lists the runtime's apps and `simctl.py launch com.apple.Preferences`
  starts Settings.
- **One clock.** Apple's code that reads the system counter itself (JavaScriptCore and VFX in the cache, any app)
  gets Darling's `mach_absolute_time` clock, as all other code does: dyld turns each such read into a trap
  libsystem_kernel answers (Darling counts nanoseconds, timebase 1/1, where Apple silicon counts 24 MHz ticks).
- **XCTest.** `scripts/xip-platform.py` takes XCTest's iPhoneSimulator frameworks out of an Xcode .xip in about a
  minute (`XCODE_XIP=... scripts/setup-arm64.sh`); Xcode's `xctest` agent runs in the device
  (`simctl.py spawn`). Running test bundles (x86Sim's `cli/libexec/xctest.py`) is next.

## What does not work yet

- **Not yet compared with Apple's simulator.** The home screen's widgets are blank white squares in a first boot,
  and the lock screen, speed and software rendering (`X86SIM_METAL=0`) are not checked yet.
- **launchd's run times** are negative (`ran for 18446742...ms`): its idea of when a job started is off.
- **No agent-sim CLI yet** on arm64: `scripts/boot-arm64.sh` boots one phone; `phone.sh`, the pool and the
  pre-translation steps are x86-only.

## How it fits together

| Piece | Where | arm64 |
|---|---|---|
| Darling | darlinghq/darling master (arm64 host support, darling#1818) + `patches/darling-arm64` | built from source |
| x86Sim's Darling changes | x86sim's forks (workloops, cooperative pool, Mach memory entries, ...) | merged into the patches |
| simhost | x86Sim `host/simhost/simhost.c` | `scripts/build-simhost-arm64.sh` |
| In-simulator helpers (simfb, simhid, simshim, metalhostd, iosurfaced, ...) | x86Sim `host/*/arm64` | prebuilt, unchanged |
| Device setup | x86Sim `cli/libexec/mksimdevice.py` | unchanged |
| mtlvk (Metal on Vulkan) | x86Sim `linux/mtlvk` + x86sim/metal2vulkan | `cargo build`, unchanged |
| simaudio (the audio devices) | x86Sim `cli/libexec/simaudio.py` | unchanged |
| The translator (arm64host, libarm64jit, AOT) | x86Sim | not used |

`patches/darling-arm64/README.md` lists every change to Darling and why.

## Running it

As root on Ubuntu 24.04 arm64 (a privileged container is enough: Darling needs its namespaces and overlayfs):

```sh
scripts/setup-arm64.sh                       # Darling, x86Sim, mtlvk, the iOS runtime (about an hour)
X86SIM_ROOT=~/x86sim scripts/boot-arm64.sh   # boots and stays up; Ctrl-C stops it
```

In another shell, with `X86SIM_ROOT=~/x86sim` and from `~/x86sim`:

```sh
python3 cli/libexec/fb2png.py /tmp/x86sim-0/x86sim-a64-simfb.ring frame.png 3   # the newest frame
python3 cli/libexec/simtouch.py home                                         # a button press
python3 cli/libexec/simctl.py launch com.apple.Preferences                   # an app
```

`X86SIM_OSLOG=SpringBoard,backboardd X86SIM_OSLOG_DIR=/Volumes/SystemRoot/<dir>` on the boot writes those programs'
`os_log` messages to files.

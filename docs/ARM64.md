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
  minute (`XCODE_XIP=... scripts/setup-arm64.sh`). x86Sim's runner (`cli/libexec/xctest.py`) runs logic tests
  (`--test-bundle B.xctest`, in Xcode's `xctest` agent) and app-hosted unit tests (`--app A.app --test-bundle
  B.xctest`, injected into the installed app) with per-test results and JUnit XML. XCTestDemo's logic and hosted
  bundles, built on Linux by rules_applecross (`tests/apps/XCTestDemo`), give the results they give in Xcode's
  simulator: every test passes except the deliberate failures, which fail with their file and line, and the skipped
  one, about 4 s for the logic bundle and 17 s for the hosted one (with the app's install) on a booted phone.
- **UI tests (XCUITest).** `xctest.py --ui-test-runner R.app --app A.app`: the runner launches the app through
  testmanagerd and drives it (finding elements through accessibility, tapping, typing). XCTestDemo's UI bundle passes
  as in Xcode's simulator (about 150 s for three tests, with the installs). `boot-arm64.sh` loads testmanagerd and the
  automation-mode jobs (on demand, as x86Sim's xctest feature does) and turns app accessibility on, as Accessibility
  Inspector does. rules_applecross builds no runner app; one is made as Xcode makes it: Xcode's
  `iPhoneSimulator.platform/Developer/Library/Xcode/Agents/XCTRunner.app` copied as `<Bundle>-Runner.app`, its
  executable renamed to `<Bundle>-Runner`, `$(WRAPPEDPRODUCTNAME)` and `$(WRAPPEDPRODUCTBUNDLEIDENTIFIER)` in its
  Info.plist replaced (`<Bundle>-Runner`, `<test bundle id>.xctrunner`), and the `.xctest` put in its `PlugIns`. The
  test runner of M3 (docs/ARM64-ROADMAP.md) will do that itself.
- **`bazel test`.** rules_apple's `ios_unit_test` and `ios_ui_test` run on the phone with
  `runner = "@agent_sim_rules//:ios_runner"` (this repository's `bazel/`), which calls `agent-sim xctest` (the
  command `scripts/setup-arm64.sh` installs): it unpacks the bundles rules_apple built, makes a UI test's runner app,
  waits for the phone if another test has it, runs them, and hands Bazel the JUnit XML (`test.xml`), the logs
  (`test.outputs`) and the status. `--test_filter=Class/test,...` selects tests, and passing results are cached as on
  a Mac. `tests/apps/XCTestDemo` is wired up: with the phone booted, `cd tests/apps/XCTestDemo && bazel test //...`,
  given a `.bazelrc.user` with `common --distdir=<folder holding Xcode_27.2.xip>` and, where the machine has no gcc,
  `common --repo_env=CC=clang`. A phone runs one test at a time; more phones run more at once: `A64_PHONE=2
  scripts/boot-arm64.sh` boots a second (`device-a64-2`, its own Darling prefix `~/.darling-2`), `agent-sim xctest`
  takes whichever is free, and `bazel test --local_test_jobs=2` keeps both busy (XCTestDemo's three targets: 133 s
  on two phones, about 200 s on one). Each phone after XCTestDemo's UI tests takes about 3.7 GB (2.8 GB of its own and
  0.9 GB of shared memory), so a 16 GB container holds three or four. If a phone stops during a test (its Darling
  server gone), the test runs once more on another booted phone; the stopped phone stays down until booted again.
  `agent-sim phones up N` boots phones 1 to N that do not run and returns once each can run tests (its SpringBoard
  up for two minutes: about two minutes after boot); `agent-sim phones status` lists them. For bundles whose failures
  are deliberate, `--expect FILE` (or `--test_env=AGENT_SIM_EXPECT=FILE`) passes a test when every test had the outcome
  FILE gives it (XCTestDemo's: `tests/apps/XCTestDemo/expected.txt`), so `bazel test //...` there passes.
- **The image** of Nimbus' `ios` leases: `images/arm64/Containerfile` (about 4.4 GB) runs `setup-arm64.sh` with
  `RUNTIME_BUILD=none` and drops Darling's sources and build tree; Darling's prefix records the patches it was built
  with, so `setup-arm64.sh` in a lease skips Darling and only adds what is missing. Apple's runtime
  (`$X86SIM_ROOT/runtimes/ios-27.2-24B8117-arm64`) and XCTest's platform (`$X86SIM_ROOT/sdks/iPhoneSimulator.platform`)
  come from outside the image. Both may be read-only (a Bazel repository's extracted archive, or a volume):
  `AGENT_SIM_RUNTIME_RO` (default `$X86SIM_ROOT/runtimes-ro/ios-27.2-24B8117-arm64`) and `AGENT_SIM_XCTEST_PLATFORM`
  name them; `boot-arm64.sh` puts an overlay over the runtime at `$X86SIM_ROOT/runtimes/<runtime>`
  (x86Sim writes each device's launch angels into the runtime's LaunchAngels directory, the only place launchd_sim
  takes angels from), and Darling keeps its rewritten-instruction lists in each phone's prefix.
- **Driving a phone from outside** (Nimbus' driver): `agent-sim --device <device dir> install <.app|.ipa>`,
  `uninstall`, `launch`, `terminate`, `apps` (x86Sim's in-device helpers underneath) and `screenshot -o out.png` (the
  frame ring). An app's first launch can take up to a minute to draw its first frame. Tap, swipe and type are next.

## What does not work yet

- **Not yet compared with Apple's simulator.** The home screen's widgets are blank white squares in a first boot,
  and the lock screen, speed and software rendering (`X86SIM_METAL=0`) are not checked yet.
- **An app crash on a first boot.** In two of about seventeen runs of XCTestDemo's UI tests, both on a phone booted
  for the first time with a new Darling prefix while another phone ran tests, the app died (SIGSEGV) or did not show
  its text field in 30 s; on that phone's next boot, ten more runs passed. Not yet caught with a core.
- **Asking a process for its accessibility tree** over x86Sim's per-process socket
  (`/tmp/x86sim-0/x86sim-ax-<UDID>-<pid>.sock`) and hanging up before it answers kills it (SIGPIPE): read the whole
  answer, or leave SpringBoard's alone.
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

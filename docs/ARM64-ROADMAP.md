# Roadmap: iOS development and tests in Linux containers

The goal: an agent in a Linux container (a Nimbus lease or a CI job) builds an iOS app and its test bundles, runs
the unit and UI tests in an iOS simulator in the same kind of container, and drives the app itself, with no Mac.

agent-sim picks up where rules_applecross leaves off: rules_applecross builds clean-room bundles on Linux, and
agent-sim runs them in a simulator that is not clean-room. The bundles are the same either way, so each project
chooses where to run them:

- **Purity:** build on Linux with rules_applecross, run in Apple's own simulator on a macOS VM. Nothing outside
  the clean room touches the project.
- **Speed and scale:** build on Linux, run on Linux with agent-sim. Phones are limited only by CPU and memory, not
  by the two-VM licence cap on macOS.

Two halves, kept apart on purpose:

| | Builds (ships) | Runs (never ships) |
|---|---|---|
| Project | rules_applecross (Apple Cross session) | agent-sim arm64 (Agent Simulator session) |
| Produces | apps, extensions, `.xctest` bundles, signed, for the App Store | a booted simulator in a container, test results |
| Method | clean room: no decompiling or disassembling Apple's binaries | may disassemble Apple's binaries (as x86Sim did) |
| Ends at | "`bazel test` off a Mac builds the bundles but has nothing to run them on" | `bazel test` runs them |

## Where things stand (2026-11-25)

Done (docs/ARM64.md): Apple's iOS 27.2 runtime boots natively in an arm64 Nimbus container under Darling, with no
translator. launchd_sim, backboardd and SpringBoard come up; SpringBoard finishes starting at the home screen; apps
launch (`simctl.py launch`); Metal works through mtlvk on lavapipe (48/49 of x86Sim's checks, the 49th a test bug).
Reproducible from scratch with `scripts/setup-arm64.sh`.

The home screen and apps draw (Settings, for one), since a fix to Darling's `mach_vm_region` for arm64's address
space; widgets, the lock screen and speed are not checked yet. Apple's code that reads the system counter gets
Darling's clock (M1.1). XCTest logic and app-hosted unit tests run (M2), and so do XCTestDemo's UI tests (M4). No
agent CLI or Bazel integration yet.

Useful already: x86Sim's XCTest runner (`cli/libexec/xctest.py`) covers logic tests (the `xctest` tool), app-hosted
unit tests (`libXCTestBundleInject`) and XCUITest (testmanagerd and XCTRunner), from an `.xctestrun` or explicit
bundles, with JUnit XML. It needs Xcode's iPhoneSimulator platform frameworks (XCTest and friends).

## Milestones

Unit tests need no pixels and UI tests need few, so M2 and M3 run alongside M1 rather than after it.

### M1. A correct simulator: clocks, then pixels

1. **One clock.** Darling's `mach_absolute_time` counts nanoseconds (timebase 1/1); `CNTVCT_EL0`, which some of
   Apple's code reads itself (JavaScriptCore and VFX in the cache: 35 reads), counts 24 MHz ticks. Rather than move
   all of Darling (client and server, which assume nanoseconds in dozens of places) to ticks, each counter read in
   Apple's code becomes a trap that answers Darling's clock, as TSD reads already do: in the cache, in `dyld_sim`,
   in every image `dyld_sim` maps and in the main executable. Do it first, because XCTest's timeouts and
   expectations run on this clock. (launchd's negative run times are a separate question.)
2. **Client layers on screen.** Done (2026-11-25): the render server checks each buffer a client sends with
   `mach_vm_region`, which refused arm64 Linux's high addresses; the 16K-page lead was not it. Originally: find
   where SpringBoard's and apps' layer trees stop. Instrument backboardd's render
   server (commits received, decoded, contexts attached to the display), with disassembly where needed. Lead: x86Sim
   presents the runtime with 16K pages (commpage page shift 14, 16K-aligned memory) and native Darling with the
   host's 4K. If that is it, arm64 Darling must present 16K pages: a commpage change plus 16K-aligned `mmap`,
   `vm_allocate` and `mach_vm_map` placement in libsystem_kernel (darlingserver's duct-tape is already 16K).
3. **Done when:** a first boot shows the lock screen and home screen with icons, and Settings, as Apple's simulator
   draws them (x86Sim's reference screenshots), at about the same speed as x86Sim on comparable hardware.

### M2. XCTest in the container: logic and app-hosted unit tests

Done (2026-11-25): XCTestDemo's logic and hosted bundles, built on Linux by rules_applecross, run in a container
with x86Sim's runner and pass as in Xcode's simulator, with JUnit XML (docs/ARM64.md).

1. Run x86Sim's `xctest.py` against an arm64 boot (it assumes `boot-a64.sh`'s layout and the translator's process
   model in places): logic bundles first, then app-hosted.
2. XCTest's platform frameworks from an Xcode `.xip` on Linux, the same Xcode rules_applecross already takes its
   SDK from (x86Sim's `dev setup sdk --xip`), fetched once per machine like the runtime, never committed.
3. Test bundles **built by rules_applecross on Linux**, not by Xcode: x86Sim's `tests/apps/XCTestDemo` (logic,
   hosted and UI targets) as a rules_apple workspace. This is the first time the two halves meet.
4. **Done when:** XCTestDemo's logic and hosted tests, built on Linux, pass in a container, with JUnit XML; a
   failing assertion fails with the right file and line.

### M3. `bazel test` on Linux

1. A test runner for rules_apple's `ios_unit_test` / `ios_ui_test` (`AppleTestRunnerInfo`, as
   `ios_xctestrun_runner` provides) that installs the bundles on a simulator, runs them, writes `$TEST_XMLFILE`
   and returns the test's status.
2. The simulator needs root, namespaces and a boot (a minute or two), none of which a Bazel test sandbox gives. So
   the runner is a thin client of a **phone service** in the container (agent-sim's pool: phones booted once,
   reset between test targets, handed out one per test), over a Unix socket.
3. The runner lives in agent-sim (or a small Bazel module published from it), not in rules_applecross, so no
   simulator-side code enters the clean-room repository. Projects using rules_applecross select it with one
   `runner =` line.
4. **Done when:** `bazel test //...` of a rules_applecross project in an arm64 container runs its iOS unit tests,
   several test targets in parallel on several phones, and Bazel's result caching works as on a Mac.

Steps 1 to 3 are done (2026-11-26): `bazel test` of XCTestDemo's three targets runs them in the container with the
results Xcode's simulator gives, with JUnit XML, `--test_filter` and result caching, on as many phones as are booted
(two tested). From step 4: a test whose phone stops mid-run runs again on another phone (tested by killing a
phone's Darling server during XCTestDemo's UI tests); `agent-sim phones up N` boots and waits for phones, and
`--expect` checks deliberate outcomes. CI on Nimbus (Logan approved an exception for this public repository,
2026-11-26) is being set up with the Nimbus session and NCH.

The plan, in the order it lands. rules_apple's test rules expand the runner's script template with the test bundle
(`%(test_bundle_path)s`, a `.zip`), the test host (`%(test_host_path)s`, an `.ipa`, or nothing for a logic test),
`%(test_type)s` (`XCTEST` or `XCUITEST`), `%(test_filter)s` and `%(test_env)s`; Bazel adds `XML_OUTPUT_FILE`,
`TESTBRIDGE_TEST_ONLY` and `TEST_UNDECLARED_OUTPUTS_DIR`.

1. **`agent-sim xctest` on one phone.** A subcommand of agent-sim's CLI (Rust, as the rest of it) that takes those
   inputs, unpacks them where the phone sees them, makes the UI runner app from Xcode's XCTRunner.app (docs/ARM64.md),
   takes the phone (a lock, so two tests never share one), runs x86Sim's `xctest.py`, uninstalls what it installed,
   copies the JUnit XML to `XML_OUTPUT_FILE` and logs to `TEST_UNDECLARED_OUTPUTS_DIR`, and exits 0 only if every
   test passed or was skipped. Bazel's `--test_filter` maps to `--only-testing`.
2. **The Bazel side.** A small module in this repository (`bazel/`): `ios_agent_sim_runner`, returning
   `AppleTestRunnerInfo` with a template of a few lines that runs `agent-sim xctest` with the substitutions. The
   test action runs outside Bazel's sandbox (`no-sandbox`), since the phone is a service of the container, and the
   `agent-sim` it calls is the container's, set up with the phones. XCTestDemo's targets select it with
   `runner =` in `tests/apps/XCTestDemo` (a BUILD file of this repository, not rules_applecross's).
3. **Several phones.** `agent-sim pool up N` boots N arm64 phones (each its own device and Darling prefix), and
   `agent-sim xctest` takes whichever is free, so `bazel test --local_test_jobs=N` runs N targets at once. Between
   targets a phone is reset by uninstalling the run's apps, not rebooted.
4. **Then:** a phone that crashes or hangs is rebooted and the target retried once; boot time and memory per phone
   measured; the XCTestDemo targets run in CI of this repository on a Nimbus container.

### M4. UI tests

Started (2026-11-25): XCTestDemo's UI tests, built on Linux by rules_applecross, pass in a container (docs/ARM64.md).
Screenshots, the held status bar and a real app's suite are still to do.

1. XCUITest: testmanagerd in the device (x86Sim's `xctest` feature), XCTRunner, the app launched and driven
   through simhid. Queries go through accessibility, so most UI tests should run once M2 works; screenshots
   (`XCUIScreenshot`, failure attachments) need M1.
2. Deterministic screenshots as AGENTS.md asks of every proof: the status bar held at 9:41 with full signal and
   battery (CoreSimulator's `status_bar override`, which x86Sim's simctl may already have), a fixed locale, time
   zone and appearance.
3. **Done when:** XCTestDemo's UI tests and one real app's UI suite (Visor's, say) pass in a container, with
   screenshots that match the same tests on a macOS VM closely enough for review.

### M5. Agents drive apps

1. agent-sim's CLI and MCP server on arm64: `up`, `install`, `launch`, `snapshot`, `tap`, `type`, `screenshot`, the
   pool and the browser stream. `phone.sh` and the pre-translation steps are x86-only today; the accessibility
   hook and the SpringBoard hook are simulator dylibs that rules_applecross can now build on Linux.
2. Fast phones: boot once and keep a booted prefix to clone, so a fresh phone takes seconds; measure phones per GB
   (x86Sim fit three in 6.8 GB with the translator; native should do better).
3. **Done when:** an agent in a Visor container session builds an app with rules_applecross, installs it, drives
   it over MCP and runs its tests, all in containers.

### M6. In Nimbus and CI

Under way (2026-11-26): Logan wants every repository's agents and CI in Linux containers, with iOS through agent-sim
as Nimbus' `ios` platform (nimbus#200). The image is `images/arm64/Containerfile` (no Apple files; built on GitHub's
arm64 runner into ghcr.io/attilathefun/agent-sim-arm64), the runtime and XCTest's platform come from a read-only Nimbus
volume, and a lease brings its phones up with `agent-sim phones up N`. agent-sim itself has no CI (Logan): it is
tested on demand in a lease with XCTestDemo and `--expect`.

1. A Nimbus config (an image with Darling built in, the runtime and Xcode's platform frameworks in Nimbus
   storage, not in the image) that leases a container with a phone service already booted. Owned with the Nimbus
   session.
2. CI: the unit and UI test jobs of private repositories move from macOS VMs to these containers (the two-VM
   licence cap stops being the bottleneck). Release proof screenshots stay on macOS VMs until M4's screenshots
   are trusted.
3. Runtime updates: each new iOS 27.x (and later 28) runtime gets a smoke boot and XCTestDemo run before CI
   switches to it.

### macOS apps

Next after Nimbus' `ios` platform: macOS's own userland under Darling, from Apple's restore image, for headless macOS
XCTest and later AppKit and SwiftUI UI tests: docs/MACOS-DARLING.md (milestones D1 to D6, and what D1 has shown).

### M7. Upstream and maintenance

The general Darling fixes (the `/proc/<pid>/mem` fallback, libnetwork on arm64, XNU's `apple[]` strings, host-page
rounding, the dyld_sim helpers) go to darlinghq as Logan's PRs, through the Upstream session. agent-sim's patches
follow x86Sim's forks as they move.

## Rules between the two halves

- **Nothing derived from Apple's binaries goes to Apple Cross**: no code, notes, hints, symbol names from
  disassembly or explanations of why Apple's code behaves as it does. What may go: whether its bundles' tests
  passed, failed or crashed, their logs and JUnit output, and stack frames in its own code. The simulator is a
  black box to rules_applecross, which may use its outputs as a reference, as the hand-off brief allows.
- **Apple Cross may send problem statements**: bundles that do not run, test projects, expectations. Answers come
  back as results, not reasons.
- **Nothing of the simulator ships.** Shipped apps are built by rules_applecross alone. The test runner installs
  and runs bundles; it never changes them, and its own code stays in agent-sim.
- Apple's simulator runtime and Xcode's platform frameworks are fetched from Apple onto each machine and never
  committed or published, as today.

## Order and dependencies

```
M1.1 clock --+--> M2 unit tests --> M3 bazel test --+--> M6 Nimbus + CI
             |                                      |
             +--> M1.2 pixels --> M4 UI tests ------+
                       |
                       +--> M5 agents drive apps ----> M6
M7 runs alongside.
```

Risks, largest first: XCUITest under Darling (testmanagerd's privileged paths); boot time and memory per phone in
CI; keeping up with new runtimes.

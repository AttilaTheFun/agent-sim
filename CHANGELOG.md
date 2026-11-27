# Changelog

## Unreleased

- arm64 Linux, natively (work in progress, docs/ARM64.md): `scripts/setup-arm64.sh` builds Darling with
  `patches/darling-arm64` (x86Sim's Darling changes merged with Darling's arm64 host support, and the fixes arm64
  needed) and x86Sim's Metal stand-in for arm64; `scripts/boot-arm64.sh` boots the iOS 27.2 runtime with no
  translator, through SpringBoard to the home screen, which backboardd draws whole, as it does apps (Settings).
  XCTest logic, app-hosted unit and UI tests run there (x86Sim's `xctest.py`), with XCTest's frameworks from an
  Xcode .xip (`XCODE_XIP`); `tests/apps/XCTestDemo` builds the test bundles on Linux with rules_applecross.
- `agent-sim xctest` runs a test bundle on the arm64 phone, and `bazel/` (the `agent_sim_rules` module) is a test
  runner for rules_apple's iOS test rules that calls it: `bazel test` of iOS unit and UI tests in a Linux container,
  on several phones (`agent-sim phones up N`), with `--expect` for bundles whose failures are deliberate.
- `agent-sim simctl` (and `xcrun simctl`, through an `xcrun` link): Xcode's simctl for the arm64 phones, the verbs
  CI scripts and Isomer's launcher use (list, create, boot, bootstatus, shutdown, delete, install, launch, terminate,
  spawn, io screenshot with `--mask`, ui appearance, keychain reset, status_bar). Visor's iOS proof runs on Linux
  through it, its pictures identical to a Mac's below the status bar but for the screen that shows a chat thread.
- `simctl status_bar override` holds the status bar as Apple's simulator does: the time (as Apple draws it, with the
  narrow space its "AM" left), cellular and Wi-Fi bars and modes, the operator and the battery's level and state.
- `simctl boot` then `bootstatus -b` boots a phone once, and `shutdown` returns once the phone is gone. Phones are
  found from the account's home directory, not `$HOME`, which `bazel test` changes.
- Phones boot x86Sim's slim job list by default (plus installcoordinationd and pasted): 35 of Apple's launchd jobs
  instead of 107, ~2.6 GB per phone running an app instead of ~3.2 GB, the same pictures and test results.
  `A64_JOB_LIST=std` boots the full list.
- SpringBoard's wallpaper posters and widget renderers are hidden (whiteouts in the runtime's overlay; the read-only
  runtime is untouched): SpringBoard starts them whatever the job list. An idle phone takes ~1.6 GB (37 processes),
  from ~2.0 GB. The home screen's wallpaper is black and its two widgets blank, as before. `AGENT_SIM_HIDE_POSTERS=0`
  keeps them.
- A test chosen with `--only-testing` (or `bazel test --test_filter`) runs once, not twice: x86Sim's run list named
  each test in two forms (patches/x86sim-arm64, which setup-arm64.sh applies).
- `phones up` returns once the phone takes an app launch (Settings, launched and closed again) and has spent the first
  touch of the boot, which was dropped: ready 3-4 minutes after boot, with taps showing in about a second.
- A phone whose boot dies (launchd_sim killed: a SIGSEGV at the start of a boot, now and then) is booted again, up to
  twice, within seconds: `phones up` and `simctl bootstatus` waited 900 s on it before.
- `setup-arm64.sh` pins every download by version and SHA-256 (rustup-init and Rust, 7-Zip, ipsw, bazelisk), and
  builds on Ubuntu 26.04 too.
- `hooks/build.sh` builds the hooks on Linux too (Swift's clang, the MacOSX SDK from Xcode's .xip, LLVM's ld64.lld).
- Web pages no longer stay blank, and `open` brings Safari up after it was quit: reading the screen sent SIGUSR2 to
  processes still starting, which ended them; now only processes whose hook is ready are asked.
- `open` checks that Safari started for a web link and asks again once if it did not.
- `phone.sh` passes Darling's `DSERVER_*` log settings through to the phone.
- `status` reports a phone whose Darling server exited as crashed, and commands that read the screen say how to
  restart it, instead of timing out.
- The README opens with the dino-game clip.

## 0.1.0

First release.

- `agent-sim` command line: `setup`, `up`, `down`, `status`, `snapshot` (the screen as accessibility elements
  with references), `tap`, `type`, `scroll`, `swipe`, `press`, `wait`, `screenshot`, `install` (.app, and .ipa
  device builds converted for the simulator), `uninstall`, `launch`, `terminate`, `apps`, `open`, `logs`, `pool`,
  `serve` (live H.264 view in a browser) and `mcp` (Model Context Protocol server).
- Setup for one command on Ubuntu 24.04: x86Sim, Apple's iOS 27.2 runtime, a pre-translated runtime, an iPhone 17
  Pro drawn at 2x, the system services apps expect, reduced effects and app accessibility.
- Fixes to x86Sim (patches/x86sim): web workers in Safari; apps with crash reporters no longer crawl.
- Hooks in the phone (hooks/): the accessibility reader, and permission prompts dismissed when their app quits.
- Several phones per VM with shared memory (KSM), and a pool of booted phones.

# Roadmap

How agent-sim goes from a working experiment to a public open-source release. Each phase lists what ships, how it
is measured, and what has to be true before the next phase starts.

## Goal

An iOS simulator on an ordinary Linux VM that feels as fast as Apple's Simulator on a Mac, driven by coding agents
through a small CLI and an MCP server, and watchable live in a browser tab.

**Non-goals for 1.0:** physical devices, App Store builds (they are encrypted and do not run in any simulator),
signing and publishing apps, macOS apps.

## Where we start

A first test on a cloud VM (8 vCPU AMD EPYC Zen 3, 16 GB, no GPU, Ubuntu 24.04) running x86Sim 0.5.2 with the
iOS 27.2 runtime and Metal on Mesa's CPU Vulkan driver (lavapipe):

| Measure | Baseline |
|---|---|
| Boot to home screen (warm translation cache) | 42 s |
| App launch, first frame / settled | Safari 0.18 s / 1.3 s, Maps 1.3 s / 2.5 s, Settings 3.7 s |
| Touch to new frame (on the VM) | 475 ms p50 |
| Settings scrolling | 40 fps, p95 frame 56 ms |
| CPU at idle | 1.1 cores |
| Memory, VM total with one phone | about 5.7 GB |
| Pre-translated code | none: every app translated its code on first use |

Through the first browser viewer (MJPEG frames, one HTTPS request per touch) it was slow enough to be hard to use.

## Targets for one device

| Measure | Target |
|---|---|
| Boot to home screen | under 15 s; resume a saved phone in under 3 s |
| App launch, warm | first frame under 0.5 s, settled under 1.5 s |
| Touch to new frame (on the VM) | under 50 ms p50 |
| Touch to pixels in a browser (same region) | under 120 ms p50 |
| Scrolling | 55+ fps, p95 frame under 25 ms |
| CPU at idle | under 0.3 cores |
| Memory, VM total | under 3.5 GB |

Every number comes from `bench/` on a named machine type, before and after each change. A change that does not move a
number, or moves one the wrong way, does not merge.

## Architecture

```
 agent / human
     │  CLI · MCP · browser
     ▼
 agent-sim (Rust, one binary)
   ├─ cli        boot, install, launch, snapshot, tap, type, screenshot, logs
   ├─ serve      on the VM: WebSocket input, H.264 video, HTTP API, auth
   ├─ mcp        the same commands as MCP tools
   ├─ ipa        device build (.ipa) → simulator build (.app)
   └─ remote     set up and drive a phone on any SSH-reachable Linux VM
     │
     ▼
 x86Sim (upstream, GPL-3.0-or-later; run as a separate program, never linked)
   Darling · dynarmic · metal2vulkan · Apple's iOS runtime (downloaded on the user's machine)
```

- **Rust** for agent-sim: one static binary, fast startup for agent calls, safe concurrency for the streamer.
  Shipped through GitHub Releases, Homebrew, `cargo install` and an npm wrapper, as Vercel Labs' agent-browser is.
- **License boundary.** agent-sim is Apache-2.0 and talks to x86Sim only through its command line, files and sockets.
  Changes inside x86Sim are GPL and go upstream as pull requests. Until they land, they live in a public fork as a
  patch series, and agent-sim pins the fork's release.

## Phases

### Phase 0: Foundations

- Repository layout: Cargo workspace (`crates/`), `bench/`, `docs/`, `scripts/`.
- CI on every push: `cargo fmt --check`, `clippy -D warnings`, unit tests, Linux x86-64 release build.
- Commit conventions: one logical change per commit, imperative subject under 72 characters, a body that says why.
- `CONTRIBUTING.md`, `SECURITY.md`, issue and pull request templates.

**Done when** CI is green on an empty CLI that prints its version.

### Phase 1: Reproducible setup and benchmarks

- `scripts/setup-vm.sh`: a fresh Ubuntu 24.04 VM to a booted phone with one command. It covers user lingering for
  `/run/user`, Mesa's Vulkan driver, the x86Sim AppImage, the runtime download and the first device.
- `bench/run`: boot time, launch times (cold and warm), scroll frame times, touch-to-frame latency, idle CPU and
  memory, written as JSON with the machine's CPU model, core count and kernel.
- `docs/BENCHMARKS.md`: the table above, kept current.

**Done when** two clean VMs of the same type give numbers within 10% of each other.

### Phase 2: One device at full speed

Work through these in order, benchmarking after each:

1. **Pre-translation.** Build x86Sim's ahead-of-time image from the training it ships plus our own app runs, so no
   system framework is translated on first use. Keep the image across boots and ship a prebuilt one with the setup.
2. **Background services.** Start from x86Sim's `slim` and `min` job lists and remove what an agent never needs
   (Siri, Spotlight indexing, iCloud, News and Maps data, analytics). Check every removal against crash logs.
3. **Pixels.** Render fewer pixels: the device's own size at a lower scale, or a smaller model when the test allows.
   Measure lavapipe thread counts and the Metal path (`X86SIM_METAL`) against software rendering.
4. **CPU placement.** Pin the phone's busiest threads (SpringBoard, backboardd, the frontmost app, mtlvk) to
   dedicated cores, use huge pages for the translation cache, and measure kernels 6.11+.
5. **Input latency.** Trace a touch from the socket to the published frame and remove the waits on that path.
   Fixes inside x86Sim go upstream.
6. **Saved phones.** Snapshot a booted phone and resume it, so a new session does not boot at all.
7. **Hardware.** Run the suite on common cloud VM types (for example AMD EPYC Zen 3 and Zen 5) and recommend one.

**Done when** the one-device targets above are met on the recommended VM type.

### Phase 3: Live view in the browser

- `agent-sim serve`: one WebSocket for input (touch, keys, buttons) and a native H.264 stream (low-latency settings,
  only changed frames) over WebSocket or WebRTC. MJPEG stays as a fallback.
- A small web viewer: the phone at its real aspect ratio, mouse and touch input, keyboard, Home and Lock, rotation
  later.
- A capability token on every URL, and TLS through the VM provider's proxy or a tunnel.

**Done when** touch to pixels in a browser is under 120 ms p50 in the same region and scrolling looks smooth.

### Phase 4: The agent interface

- CLI commands, each one fast and with JSON output: `boot`, `install`, `launch`, `terminate`, `snapshot`, `tap`,
  `swipe`, `type`, `press`, `screenshot`, `openurl`, `logs`.
- `snapshot`: the screen's accessibility tree as compact text with element references (`@e3`) that `tap` accepts,
  so agents act without screenshots. This needs a way to read the accessibility hierarchy inside x86Sim; it is the
  main research item of this phase.
- `agent-sim mcp`: the same commands as MCP tools. A `SKILL.md` that teaches any agent the CLI.

**Done when** an agent installs a sample app, signs in through its form and checks the result using only `snapshot`
and the input commands.

### Phase 5: Apps

- Install `.app` simulator builds (arm64), including Expo Go and Expo development clients with Metro on the VM.
- `agent-sim ipa`: turn an unsigned or development `.ipa` into a simulator build by rewriting each Mach-O's platform
  load command to iOS Simulator and re-signing ad hoc, then report frameworks that will not load in a simulator.
- A compatibility list of open-source apps that run, with what breaks for the ones that do not.

**Done when** ten public sample apps (UIKit, SwiftUI, React Native, Expo, Flutter) install and launch.

### Phase 6: Remote and cloud

- `agent-sim remote`: set up and drive a phone on any Linux VM reachable over SSH.
- Ready-made images or setup scripts for common providers, and a guide for running one on your own VM.

**Done when** a new user goes from nothing to a phone in their browser in under 15 minutes on a fresh VM.

### Phase 7: More than one device

- Fix what breaks with two phones on one VM (in the first test the second boot froze the first phone's screen, and
  touch did not reach the second phone).
- Shared memory across phones, per-phone CPU limits, and the number of phones per VM size.

### Phase 8: Hardening

- End-to-end tests on a real VM, nightly: setup, boot, install, launch, input, stream.
- Clear errors for the common failures: no Vulkan driver, blocked user namespaces, too little disk or memory.
- No telemetry by default.

## Going public

1. **Legal review.** Confirm the GPL boundary, the README's statements about Apple's runtime (downloaded by the user
   from Apple, never redistributed), and nominative use of Apple's names. No Apple logos or artwork.
2. **History check.** Read the full commit history and every file for secrets, private hosts and personal data.
3. **README** with a short demo video, install in one command, the benchmark table, credits and limitations.
4. **Docs**: setup, CLI reference, MCP, viewer, troubleshooting, architecture, how to contribute upstream.
5. **Releases**: signed Linux binaries, Homebrew tap, crates.io, npm wrapper, a changelog, semantic versions.
6. **Community**: issue templates, a code of conduct, a security policy, labels for good first issues.
7. **Launch**: make the repository public, publish 0.1.0, then announce with the benchmark numbers and the demo.

## Risks

| Risk | What we do |
|---|---|
| x86Sim changes or stops | pin releases; keep our fork's patch series small and upstreamed |
| Apple's terms | never ship Apple software; the user downloads the runtime from Apple; get a legal read before 1.0 |
| CPU rendering caps frame rate | fewer pixels, the Metal path, and GPU VMs as an option |
| Accessibility tree not reachable | fall back to on-device OCR plus screenshots until it is |
| Device builds that need device-only frameworks | report them clearly; simulator builds stay the main path |

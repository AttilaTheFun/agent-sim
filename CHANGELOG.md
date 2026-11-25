# Changelog

## Unreleased

- arm64 Linux, natively (work in progress, docs/ARM64.md): `scripts/setup-arm64.sh` builds Darling with
  `patches/darling-arm64` (x86Sim's Darling changes merged with Darling's arm64 host support, and the fixes arm64
  needed) and x86Sim's Metal stand-in for arm64; `scripts/boot-arm64.sh` boots the iOS 27.2 runtime with no
  translator, through SpringBoard to the home screen, which backboardd draws whole, as it does apps (Settings).
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

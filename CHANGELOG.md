# Changelog

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

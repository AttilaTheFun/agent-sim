# Hooks

Small libraries agent-sim loads into the phone's own processes to fix behavior that differs from a device. They are
built for the iOS simulator from plain C against the Objective-C runtime, without an iOS SDK: `hooks/build.sh`
compiles for arm64 macOS and marks the result as an iOS-simulator binary (`retag.py`). It runs in an `ios` lease
(Swift's clang, the MacOSX SDK from Xcode's .xip, LLVM's ld64.lld; see the script) or on a Mac with Xcode's command
line tools. The built copies are checked in under `prebuilt/`; rebuild them with every change to the sources.

| Library | Loaded into | What it does |
|---|---|---|
| `libagentsim-springboard.dylib` | SpringBoard | Cancels an alert whose poster died, and a permission prompt whose app quit (Darling loses the dead-name notifications SpringBoard and tccd wait for; see `docs/KNOWN_ISSUES.md`) |

`scripts/phone.sh` loads them through x86Sim's per-program environment file (`AGENT_SIM_HOOKS=0` turns them off,
`AGENT_SIM_HOOK_LOG=1` logs to `/tmp/agent-sim-springboard.log`).

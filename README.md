# agent-sim

iOS simulators for AI agents, on ordinary Linux machines.

agent-sim runs Apple's iOS simulator runtime on x86-64 Linux and gives coding agents a small command line to drive
it: boot a device, install an app, read the screen as an accessibility tree, tap, type and take screenshots. The goal
is iOS testing on cheap Linux VMs instead of rented Macs.

> **Status: early.** It works end to end on one VM; expect rough edges.

## Set up a machine

On a fresh Ubuntu 24.04 x86-64 machine (8+ CPUs, 16 GB, 60 GB free disk; no GPU needed):

```sh
scripts/setup-vm.sh
scripts/phone.sh start && scripts/phone.sh wait
```

The setup downloads x86Sim and Apple's iOS 27.2 runtime (about 8 GB, from Apple), creates an iPhone 17 Pro drawn at
2x, builds x86Sim's translator with the fixes in [patches/x86sim](patches/x86sim), and pre-translates the runtime.
It takes about half an hour once; after that a phone boots in 12–30 seconds, depending on the CPU.
[docs/BENCHMARKS.md](docs/BENCHMARKS.md) has the measurements behind these choices.

## Keep phones ready

```sh
scripts/pool.sh up 3        # prepare and boot pool1..pool3
phone=$(scripts/pool.sh take)
# ... use $phone ...
scripts/pool.sh release "$phone"   # restarts it in the background for the next taker
```

An agent gets a phone already at its home screen. With two or more, phones share memory through the kernel's KSM:
three use 6.8 GB together instead of 8.7 GB.

## Watch a phone in the browser

On the same machine, with the phone running:

```sh
cargo build --release
./target/release/agent-sim serve --device p
```

It prints a URL with an access token. The page streams the phone's screen as H.264 over a WebSocket, decoded with
WebCodecs (a recent Chrome, Edge or Safari), and sends touches, keys and the Home and Lock buttons back. Touches that
start at a screen edge are system gestures, so swiping up from the bottom goes home. `--listen 0.0.0.0:8080` serves
it beyond the machine; keep the token private.

## Drive it from an agent

```sh
agent-sim up                       # boot the phone, wait for its home screen
agent-sim install MyApp.ipa        # a device build is converted for the simulator; a simulator .app works too
agent-sim launch com.example.myapp
agent-sim snapshot                 # the screen as text, one element per line
agent-sim tap @e4                  # or: agent-sim tap "Sign in"   or: agent-sim tap 201 320
agent-sim type "hello@example.com"
agent-sim scroll down
agent-sim wait "Welcome" --timeout 20
agent-sim screenshot -o screen.png
```

`snapshot` reads the accessibility tree of the app in front and of SpringBoard (system alerts, the home screen,
the status bar), so an agent works from text instead of pixels:

```
app com.apple.Preferences
@e1 button "Apple Account, Sign in to access your iCloud data, the App Store, Apple services, and more." (201,213)
@e2 button "General" id="com.apple.settings.general" (201,320)
@e3 button "Accessibility" id="com.apple.settings.accessibility" (201,372)
...
```

Every command takes `--device NAME` (or `AGENT_SIM_DEVICE`); `agent-sim --help` lists them all (`press`, `swipe`,
`open`, `apps`, `terminate`, `uninstall`, `status`, `down`, `serve`). `snapshot --json` gives frames, ids and states.

Device builds run when they are not encrypted: App Store downloads are (FairPlay), so use an Ad Hoc, Development or
Enterprise export, or build for the simulator.

See [docs/ROADMAP.md](docs/ROADMAP.md) for what comes next.

## Credits

agent-sim builds on [x86Sim](https://github.com/x86sim/x86sim), which runs Apple's simulator runtimes on Linux
through [Darling](https://github.com/darlinghq/darling), [dynarmic](https://github.com/azahar-emu/dynarmic) and
[metal2vulkan](https://github.com/steelbrain/metal2vulkan). Fixes agent-sim needs in x86Sim itself live in
[patches/x86sim](patches/x86sim) until they are upstream.

Apple's runtimes are downloaded from Apple onto your own machine and are never part of this repository.

## License

[Apache-2.0](LICENSE), except the patches in `patches/x86sim`, which follow x86Sim's GPL-3.0-or-later.

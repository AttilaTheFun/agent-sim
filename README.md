# agent-sim

iOS simulators for AI agents, on ordinary Linux machines.

agent-sim runs Apple's iOS simulator runtime on x86-64 Linux and gives coding agents a small command line to drive
it: boot a device, install an app, read the screen as an accessibility tree, tap, type and take screenshots. The goal
is iOS testing on cheap Linux VMs instead of rented Macs.

> **Status: experimental.** Nothing here is ready to use yet.

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

## Planned

- `agent-sim boot`, `install`, `launch`, `snapshot`, `tap`, `type`, `screenshot`
- Accessibility snapshots with element references, so agents act without screenshots
- Running device builds (`.ipa`) in the simulator where possible
- Several simulators per VM, and a remote mode for cloud VMs

See [docs/ROADMAP.md](docs/ROADMAP.md) for the plan to 1.0.

## Credits

agent-sim builds on [x86Sim](https://github.com/x86sim/x86sim), which runs Apple's simulator runtimes on Linux
through [Darling](https://github.com/darlinghq/darling), [dynarmic](https://github.com/azahar-emu/dynarmic) and
[metal2vulkan](https://github.com/steelbrain/metal2vulkan). Fixes agent-sim needs in x86Sim itself live in
[patches/x86sim](patches/x86sim) until they are upstream.

Apple's runtimes are downloaded from Apple onto your own machine and are never part of this repository.

## License

[Apache-2.0](LICENSE), except the patches in `patches/x86sim`, which follow x86Sim's GPL-3.0-or-later.

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
2x, and pre-translates the runtime. It takes about half an hour once; after that a phone boots in about 40 seconds.
[docs/BENCHMARKS.md](docs/BENCHMARKS.md) has the measurements behind these choices.

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
[metal2vulkan](https://github.com/steelbrain/metal2vulkan).

Apple's runtimes are downloaded from Apple onto your own machine and are never part of this repository.

## License

[Apache-2.0](LICENSE)

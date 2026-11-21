# agent-sim

iOS simulators for AI agents, on ordinary Linux machines.

agent-sim runs Apple's iOS simulator runtime on x86-64 Linux and gives coding agents a small command line to drive
it: boot a device, install an app, read the screen as an accessibility tree, tap, type and take screenshots. The goal
is iOS testing on cheap Linux VMs instead of rented Macs.

> **Status: experimental.** Nothing here is ready to use yet.

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

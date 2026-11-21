# Contributing

agent-sim is early. Issues with a clear reproduction, benchmark results from new machine types and small focused pull
requests are all welcome.

## Building

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

Running a phone needs an x86-64 Linux machine. See [docs/ROADMAP.md](docs/ROADMAP.md) for where the project is going.

## Changes

- One logical change per pull request, with a description that says why.
- Commit subjects in the imperative, under 72 characters ("Add touch latency benchmark", not "added stuff").
- Performance changes include benchmark numbers from before and after, and the machine they ran on.
- Changes to x86Sim itself belong upstream in [x86sim/x86sim](https://github.com/x86sim/x86sim), under its license.

## License

By contributing you agree that your contributions are licensed under [Apache-2.0](LICENSE).

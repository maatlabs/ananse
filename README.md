<div align="center">
  <h1>Ananse</h1>
  <h2>A WebAssembly-native zero-knowledge virtual machine (zkVM)</h2>
  <br />
</div>

<div align="center">
<br />

[![CI](https://github.com/maatlabs/ananse/workflows/CI/badge.svg)](https://github.com/maatlabs/ananse/actions)
[![License](https://img.shields.io/crates/l/ananse.svg)](https://github.com/maatlabs/ananse#license)
[![Crates.io](https://img.shields.io/crates/v/ananse.svg)](https://crates.io/crates/ananse)
[![MSRV](https://img.shields.io/crates/msrv/ananse.svg)](https://crates.io/crates/ananse)
[![Releases](https://img.shields.io/github/v/release/maatlabs/ananse)](https://github.com/maatlabs/ananse/releases)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-ff69b4.svg?style=flat-square)](https://github.com/maatlabs/ananse/blob/main/CONTRIBUTING.md)

</div>

**WARNING:** This is a research project. It has not been audited and may contain bugs and security flaws. This implementation is NOT ready for production use.

## Overview

_Ananse_ (the Akan/Twi word for spider) is a WebAssembly-native virtual machine whose execution can be proved under a zero-knowledge STARK. Any program that compiles to the integer subset of WASM---written in Rust, C, C++, AssemblyScript, or any other language with a WASM target---runs on Ananse and produces a cryptographic proof that a third party can verify without re-executing the program.

## Status

Ananse is currently at version `0.1.1`. The current code is the v0.1.0 minimal WASM runtime preserved verbatim under `crates/ananse/`; it executes a curated set of integer-only WAT fixtures (the most complex of which is `fibonacci.wat`) but produces no proofs. The foundational ZK release is **v0.2.0**, in active development.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.89 or later (with `rustup`)---the minimum supported Rust version, declared as `rust-version` in the workspace manifest
- Cargo (comes with Rust)

### Installation

Build from source:

```bash
git clone https://github.com/maatlabs/ananse.git
cd ananse
cargo build --release
```

The `ananse` binary is produced at `target/release/ananse`. v0.2.0 will publish to `crates.io` for a `cargo install`.

### Running the Example

The current binary executes the bundled `hello_world.wasm` fixture under the v0.1.0 runtime:

```bash
cargo run --release
```

### Development

```bash
cargo +nightly fmt
cargo clippy --all-features --all-targets --workspace -- -D warnings
cargo build --release --all-features --all-targets
cargo doc --all-features --no-deps --document-private-items --workspace
cargo test --all-features --all-targets --workspace
```

## Architecture

The current `crates/ananse/` package contains the v0.1.0 runtime: a `nom`-based WASM binary decoder under `src/binary/`, a tree-walking interpreter under `src/execution/`, and a minimal `fd_write` WASI shim. The decoder and interpreter together cover enough of WASM to execute the integer fixtures shipped under `crates/ananse/fixtures/` (including `fibonacci.wat`) but produce no proofs.

The foundational ZK release replaces this implementation crate-by-crate. The target workspace shape is:

```txt
crates/
|-- ananse/              # binary + thin glue; depends on every workspace crate
|-- ananse_decoder/      # wasmparser-backed module validator + WASM rejection rules
|-- ananse_lift/         # static stack-to-register lift; depth-indexed register schedule
|-- ananse_executor/     # schedule-driven interpreter + StepObserver
|-- ananse_trace/        # WASM-trace builder + access-log preprocessing
|-- ananse_air/          # winter-air::Air for the WASM integer subset
|-- ananse_prover/       # winter-prover wrapper + Receipt type
+-- ananse_wasi/         # minimal deterministic wasi_snapshot_preview1
fixtures/                # WAT/WASM fixture pool, consumed by the tests crate
tests/                   # `ananse_tests` workspace member: shared helpers + integration tests
```

## Contributing

Thank you for your interest in contributing to this project! All contributions large and small are actively accepted. To get started, please read the [contribution guidelines](./CONTRIBUTING.md). A good place to start would be [Good First Issues](https://github.com/maatlabs/ananse/labels/good%20first%20issue).

## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this codebase by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Roadmap

| Milestone  | Focus                                                                                       | Status          |
| ---------- | ------------------------------------------------------------------------------------------- | --------------- |
| **v0.1.x** | Minimal WASM runtime + workspace bootstrap                                                  | **Complete**    |
| **v0.2.0** | Foundational ZK release: decode -> execute -> trace -> prove -> verify, integer WASM subset | **In Progress** |
| **v0.3.0** | Per-opcode-class AIR chips with `logup`, segmented continuations                            | Planned         |
| **v0.4.0** | Recursion, precompile chips (Keccak, SHA-256, Poseidon, Ed25519, secp256k1)                 | Planned         |

## Acknowledgments

Ananse's v0.1.0 implementation was based on Hiroki Sakamoto's [Writing a WASM Runtime in Rust](https://skanehira.github.io/writing-a-wasm-runtime-in-rust/) and the accompanying [tiny-wasm-runtime](https://github.com/skanehira/tiny-wasm-runtime) repository. That code is preserved under `crates/ananse/src/{binary,execution}/` for v0.1.x and is replaced by the foundational ZK pipeline at v0.2.0.

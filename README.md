<div align="center">
  <h1>Ananse</h1>
  <img src="./assets/ananse-wasm-zkvm.png" alt="Logo" height="200" width="200">
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

_Ananse_ (the Akan/Twi word for _spider_) is a WebAssembly-native virtual machine designed so that its execution can be proved under a zero-knowledge STARK. It is built to prove WebAssembly bytecode _directly_: the verifier is convinced the actual module ran, with no compiler in the trusted base. Any program that compiles to the integer subset of WASM---written in Rust, C, C++, AssemblyScript, or any other language with a WASM target---runs on Ananse.

The proof shape is the distinguishing choice. Ananse lifts WebAssembly's operand stack, locals, globals, and linear memory into one flat, statically-addressed register space and reasons about it with a register-shaped AIR over the Goldilocks field, so it keeps WASM as the directly-proven guest without paying a stack machine's separate permutation argument for each memory bank. The proof system is a FRI-based STARK (transparent, no trusted setup, post-quantum secure).

This release is the executable virtual machine: a module decodes, lifts to the static register form, and runs to its result and output journal, end to end, through a deterministic minimal WASI. The register-shaped AIR that a run is proved against is built and constraint-checked in the tree; the FRI prover that turns a run into a verifiable receipt lands in the next release.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.93 or later (with `rustup`)---the minimum supported Rust version, declared as `rust-version` in the workspace manifest
- Cargo (comes with Rust)

### Installation

Install the latest release directly from [crates.io](https://crates.io/crates/ananse):

```bash
cargo install ananse
```

Or build from source:

```bash
git clone https://github.com/maatlabs/ananse.git
cd ananse
cargo build --release
```

> **Note (source builds):** When running from a source build instead of `cargo install`, substitute `cargo run --release --` for `ananse` in all commands below (e.g., `cargo run --release -- run examples/fibonacci.wat 10`).

### Running a Program

`ananse run` decodes, lifts, and executes a `.wasm` or `.wat` module, printing its result (and any WASI journal):

```bash
# Recursive Fibonacci: the auto-selected export runs with one argument.
ananse run examples/fibonacci.wat 10                 # -> 89

# A named export with two arguments.
ananse run examples/gcd.wat --invoke gcd 1071 462    # -> 21

# CRC-32 over the "123456789" check vector bundled in the module.
ananse run examples/crc32.wat --invoke crc32 0 9     # -> 3421780262
```

The [`examples/`](./examples) directory collects curated, ZK-themed integer programs (a Fibonacci recurrence, a factorial and GCD, an FNV-1a hash round, modular exponentiation, a CRC-32 checksum, and a Merkle-path fold).

### Development

```bash
cargo +nightly fmt
cargo clippy --all-features --all-targets --workspace -- -D warnings
cargo build --release --all-features --all-targets
cargo doc --all-features --no-deps --document-private-items --workspace
cargo test --all-features --all-targets --workspace
```

## Architecture

The zkVM is a workspace of focused crates, each with its own README. A module flows left to right: decoded and validated, lifted to the static register schedule, executed to a record stream, and materialized into the trace the register-shaped AIR constrains.

### Crate Organization

| Crate               | Description                                                        |
| ------------------- | ------------------------------------------------------------------ |
| [`ananse`]          | CLI + umbrella library (Runtime facade, prelude)                   |
| [`ananse_decoder`]  | `wasmparser`-backed validator + integer-subset rejection rules     |
| [`ananse_lift`]     | Static stack-to-register lift: per-program-point register schedule |
| [`ananse_executor`] | Schedule-driven interpreter + step records                         |
| [`ananse_trace`]    | Unified value-bus + address-sorted access-log trace                |
| [`ananse_air`]      | Register-shaped AIR (p3-air) for the WebAssembly integer subset    |
| [`ananse_wasi`]     | Deterministic minimal wasi_snapshot_preview1                       |
| [`examples`]        | Production-grade ZK-themed integer WebAssembly programs            |
| [`tests`]           | Shared helpers, fixtures, integration tests                        |

`ananse_lift` is the architectural centerpiece: the static analysis that turns WASM's stack typing into the register schedule every downstream crate consumes, and what makes a register-shaped proof of WebAssembly possible without compiling WASM away. The FRI STARK prover that produces and checks receipts against `ananse_air` lands in the next release. The proving path is built on [Plonky3](https://github.com/Plonky3/Plonky3) (`p3-goldilocks`, `p3-air`, `p3-fri`).

## Contributing

Thank you for your interest in contributing to this project! All contributions large and small are actively accepted. To get started, please read the [contribution guidelines](./CONTRIBUTING.md). A good place to start would be [Good First Issues](https://github.com/maatlabs/ananse/labels/good%20first%20issue).

## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this codebase by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Security

All crates enforce `#![forbid(unsafe_code)]`. The zkVM has been hardened against adversarial input with resource limits, checked arithmetic, and safe type conversions. Field element arithmetic relies on Plonky3's sound implementations. See [`SECURITY.md`](./SECURITY.md) for the full threat model.

## Roadmap

Ananse's development follows a phased milestone plan.

| Release    | Focus                                                                                                                       | Status      |
| ---------- | --------------------------------------------------------------------------------------------------------------------------- | ----------- |
| **v0.1.x** | Minimal WASM runtime + workspace bootstrap                                                                                  | Complete    |
| **v0.2.0** | Executable VM: decode -> lift -> execute -> journal over the integer subset, CLI, register-AIR built and constraint-checked | **Current** |
| **v0.3.0** | FRI STARK prover and `prove` / `verify` CLI subcommands                                                                     | Planned     |

## Status

The current version is `0.2.0`. It executes any program in the integer subset of WebAssembly-MVP through the full `decode -> lift -> execute -> journal` pipeline and exposes it behind the `ananse run` command and a library `Runtime`. It does not yet produce proofs: the FRI STARK prover and the `prove` / `verify` flow are the focus of the next release.

## Disclaimer

Early adopters should be aware that Ananse `0.2.0` is a step toward Ananse 1.0, for which a formal audit process is expected. In the meantime, we invite you to explore and experiment with the project, but we do not recommend using it to build mission-critical systems.

## Acknowledgments

Ananse's original v0.1.0 runtime was inspired by Hiroki Sakamoto's [Writing a WASM Runtime in Rust](https://skanehira.github.io/writing-a-wasm-runtime-in-rust/) and the accompanying [tiny-wasm-runtime](https://github.com/skanehira/tiny-wasm-runtime) repository. That code has since been fully replaced by the register-shaped ZK architecture described above.

---

[`ananse`]: ./crates/ananse/README.md
[`ananse_decoder`]: ./crates/ananse_decoder/README.md
[`ananse_lift`]: ./crates/ananse_lift/README.md
[`ananse_executor`]: ./crates/ananse_executor/README.md
[`ananse_trace`]: ./crates/ananse_trace/README.md
[`ananse_air`]: ./crates/ananse_air/README.md
[`ananse_wasi`]: ./crates/ananse_wasi/README.md
[`examples`]: ./examples/README.md
[`tests`]: ./tests/README.md

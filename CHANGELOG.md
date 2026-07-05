# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.0] - 2026-07-05

A ground-up rebuild from the v0.1.x tutorial runtime into a WebAssembly-native zero-knowledge virtual machine. Ananse now executes any program in the integer subset of WebAssembly-MVP end to end through a register-shaped engine built for STARK proving, exposed behind a new `ananse run` command and a library `Runtime`. This release executes and journals; it does not yet produce proofs---the FRI STARK prover and the `prove` / `verify` flow are the focus of the next release.

### Added

- **A workspace of focused, independently documented crates:** `ananse_decoder` (validating WebAssembly frontend), `ananse_lift` (static stack-to-register lift), `ananse_executor` (schedule-driven interpreter), `ananse_trace` (unified access-log trace), `ananse_air` (register-shaped AIR), `ananse_wasi` (deterministic minimal WASI), and `ananse` (the CLI and library facade).
- **The `ananse run` command.** Decodes, lifts, and executes a `.wasm` or `.wat` module, printing its result and any WASI journal. Supports `--invoke <export>`, positional integer arguments (signed or unsigned decimal, or `0x` hex), and a `--verbose` execution summary. A module exporting `_start` runs in command mode and adopts the guest's `proc_exit` status.
- **A library facade.** The `ananse` crate re-exports every member crate under a short module and adds a `Runtime` (`instantiate` / `instantiate_with_wasi` / `call`) plus a `prelude`, so a caller has one entry point instead of threading the pipeline by hand.
- **The full integer subset of WebAssembly-MVP**, executed directly: `i32` / `i64` arithmetic (including `mul`, `div`, `rem`), comparisons, bitwise and bit-counting operators, conversions, locals, globals, linear memory, structured control flow, and recursive function calls.
- **A register-shaped AIR over the Goldilocks field**, built on [Plonky3](https://github.com/Plonky3/Plonky3), that a run is constraint-checked against: unified value-bus and address-sorted access-log consistency, byte-table range checks, a boundary argument binding fresh reads to public inputs, the program-ROM control-flow lookup, and the per-operator value relations. Built ahead of the prover.
- **A deterministic minimal WASI** (`fd_write`, `proc_exit`) that never touches the operating system, keeping every run reproducible.
- **Curated example programs** under `examples/`: a Fibonacci recurrence, factorial, greatest common divisor, an FNV-1a hash round, modular exponentiation, a CRC-32 checksum, and a Merkle-path fold.
- **A `cargo deny` policy** (`deny.toml`) and a minimum-supported-Rust-version check in CI.

### Changed

- **The proving path is built on Plonky3** (a FRI-based STARK over Goldilocks): post-quantum, transparent (no trusted setup), and multi-chip-native.
- **`i64` values round-trip losslessly.** Integers are carried in a two-limb field encoding, so all `2^64` `i64` values are represented faithfully.
- **Test inputs are reorganized:** showcase programs live in `examples/`; small opcode-family fixtures live under `tests/fixtures/`.

### Removed

- **The v0.1.0 tutorial runtime**---the `nom`-based binary decoder, the tree-walking interpreter, and the file-descriptor WASI shim---along with its now-unused dependencies and the reserved Winterfell dependency pins. Its entire public surface is re-implemented by the new crates.

### Security

- `#![forbid(unsafe_code)]` in every crate; checked arithmetic and `TryFrom` at every integer boundary; no wall-clock, thread identity, or nondeterministic syscalls in the execution path. Nondeterministic WASI imports are rejected at decode time, so a minimal host surface is enforced structurally. The security policy and threat model live in `SECURITY.md`.

---

## [0.1.1] - 2026-05-08

Workspace bootstrap. The single-package layout is converted into a Cargo workspace. No behavioural change relative to v0.1.0; every test from v0.1.0 continues to pass under the new layout.

### Added

- `[workspace]` with explicit `members = ["crates/ananse"]` and matching `default-members` (with `crates/ananse_*` reserved for future crates).
- `[workspace.package]` carrying `version`, `edition = "2024"`, `license`, `repository`, `homepage`, `readme`; consumed by per-crate `Cargo.toml`s via `field.workspace = true`.
- `[workspace.dependencies]` pinning shared deps. Includes a `winterfell` family pin (`winter-air`, `winter-crypto`, `winter-math`, `winter-prover`, `winter-verifier` all at `0.13`) reserved for future work.
- `[profile.test]` and `[profile.bench]` with `debug-assertions = false`; Winterfell's prover ships an over-strict `debug_assert_eq!` on per-constraint quotient degree that fires when under-exercised opcode-class selectors interpolate to the zero polynomial; soundness still holds because FRI checks the inequality, but `cargo test` would otherwise abort under the default `dev` profile.

### Changed

- All v0.1.0 source moved from `src/` to `crates/ananse/src/` (binary + library). All v0.1.0 fixtures moved from `fixtures/` to `crates/ananse/fixtures/`. The binary still runs `ananse` and the test suite still exercises the same 23 cases.
- Workspace manifest version bumped to `0.1.1`.

### Fixed

- `crates/ananse/src/execution/runtime.rs:188` -- replaced `loop { let Some(frame) = ... else { break }; ... }` with a `while let` loop. Surfaced by Rust 1.95 / clippy's `manual-while-let-some` rule.
- `crates/ananse/src/execution/store.rs:124` -- removed a redundant `.into_iter()` call on a value already implementing `IntoIterator`. Surfaced by clippy's `useless-conversion` rule.

---

## [0.1.0] - 2025-12-30

Initial release of the Maat Virtual Machine (Ananse), a WASM-based zero-knowledge virtual machine.

### Added

- WASM binary decoder supporting:
  - Module structure (preamble, sections)
  - Functions (parameters, return values, local variables)
  - Memory declarations and data sections
  - Import/export declarations
- Runtime execution engine with:
  - Basic instruction execution (`i32.const`, `i32.add`, `i32.sub`, `i32.lt_s`, `i32.store`)
  - Local variable operations (`local.get`, `local.set`)
  - Control flow (`if`, `block`, `loop`, `br`, `br_if`)
  - Function calls (internal, exported, and imported functions)
  - Memory operations (initialization and `i32.store`)
- WASI snapshot preview1 support:
  - `fd_write` implementation for stdout/stderr
- Comprehensive test suite covering:
  - Binary decoding
  - Instruction execution
  - Complex programs (Fibonacci sequence)
- Project infrastructure:
  - CI/CD workflows (build, test, lint, documentation)
  - Security audit workflow
  - Issue and PR templates
  - Code of conduct and contribution guidelines
  - Dual licensing (MIT/Apache-2.0)

## Guidelines for Contributors

When adding entries to this changelog for future releases:

1. **Format**: Follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
2. **Categories**: Use Added, Changed, Deprecated, Removed, Fixed, Security
3. **Audience**: Write for users, not developers (focus on impact, not implementation)
4. **Links**: Add comparison links at the bottom: `[0.2.0]: https://github.com/maatlabs/ananse/compare/v0.1.1...v0.2.0`

[0.2.0]: https://github.com/maatlabs/ananse/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/maatlabs/ananse/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/maatlabs/ananse/releases/tag/v0.1.0

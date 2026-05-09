# Changelog

All notable changes to this project will be documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The open dev cycle for v0.2.0, the foundational ZK release.

### Added

- **`mvm_decoder` crate.** WASM frontend that produces a validated `Module` value (validated bytes + extracted import / export metadata) for downstream consumers. Backed by `wasmparser 0.248`. Two-pass design: pass 1 drives `wasmparser::Validator` under a restricted `WasmFeatures` profile (only `FLOATS` + `MUTABLE_GLOBAL` enabled, so SIMD, threads, GC, multi-memory, reference types, tail calls, exceptions are rejected at validation); pass 2 walks the same bytes to surface domain-specific rejections. Public surface: `Module`, `ImportEntry`, `ExportEntry`, `ExportKind`, `WASI_MODULE`, and `DecodeError` with five span-bearing variants (`InvalidBinary`, `ValidationFailed`, `FloatsDisabled`, `ForbiddenImportModule`, `ForbiddenWasiImport`). Crate compiles with `--no-default-features --features alloc` so a future `no_std` verifier path is not foreclosed.

### Changed

- **Test layout restructured.** Integration tests moved out of per-crate `tests/` directories into a top-level `tests/` workspace member (`mvm_tests`, `publish = false`) sitting at the same level as `crates/`, with shared fixture-loading helpers in `tests/src/lib.rs`. `fixtures/` moved up to be a sibling of `crates/`, becoming the single fixture pool for the workspace.

---

## [0.1.1] - 2026-05-08

Workspace bootstrap. The single-package layout is converted into a Cargo workspace. No behavioural change relative to v0.1.0; every test from v0.1.0 continues to pass under the new layout.

### Added

- `[workspace]` with explicit `members = ["crates/mvm"]` and matching `default-members` (with `crates/mvm_*` reserved for future crates).
- `[workspace.package]` carrying `version`, `edition = "2024"`, `license`, `repository`, `homepage`, `readme`; consumed by per-crate `Cargo.toml`s via `field.workspace = true`.
- `[workspace.dependencies]` pinning shared deps. Includes a `winterfell` family pin (`winter-air`, `winter-crypto`, `winter-math`, `winter-prover`, `winter-verifier` all at `0.13`) reserved for future work.
- `[profile.test]` and `[profile.bench]` with `debug-assertions = false`; Winterfell's prover ships an over-strict `debug_assert_eq!` on per-constraint quotient degree that fires when under-exercised opcode-class selectors interpolate to the zero polynomial; soundness still holds because FRI checks the inequality, but `cargo test` would otherwise abort under the default `dev` profile.

### Changed

- All v0.1.0 source moved from `src/` to `crates/mvm/src/` (binary + library). All v0.1.0 fixtures moved from `fixtures/` to `crates/mvm/fixtures/`. The binary still runs `mvm` and the test suite still exercises the same 23 cases.
- Workspace manifest version bumped to `0.1.1`.

### Fixed

- `crates/mvm/src/execution/runtime.rs:188` -- replaced `loop { let Some(frame) = ... else { break }; ... }` with a `while let` loop. Surfaced by Rust 1.95 / clippy's `manual-while-let-some` rule.
- `crates/mvm/src/execution/store.rs:124` -- removed a redundant `.into_iter()` call on a value already implementing `IntoIterator`. Surfaced by clippy's `useless-conversion` rule.

---

## [0.1.0] - 2025-12-30

Initial release of the Maat Virtual Machine (MVM), a WASM-based zero-knowledge virtual machine.

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

[0.1.1]: https://github.com/maatlabs/mvm/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/maatlabs/mvm/releases/tag/v0.1.0

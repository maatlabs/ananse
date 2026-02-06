# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/maatlabs/mvm/releases/tag/v0.1.0

# ananse

Ananse CLI and library --- a WebAssembly-native zero-knowledge virtual machine (zkVM).

## Overview

As a binary `ananse` runs WebAssembly modules through the decode -> lift -> execute pipeline; as a library it re-exports every member crate under a short module (`decoder`, `lift`, `executor`, `trace`, `air`, `wasi`) and adds a `Runtime` facade over the whole pipeline plus a `prelude`. Programs in the integer subset of WebAssembly-MVP execute deterministically through a minimal, side-effect-free WASI; the STARK `prove` / `verify` flow arrives with the prover in a later release.

## Subcommands

```sh
ananse run <module.wasm|.wat> [--invoke <export>] [args...] [--verbose]
    Decode, lift, and execute a module, writing its journal and result.
```

`ananse run` loads a `.wasm` or `.wat` module and runs an entry to completion:

- With no `--invoke`, a module exporting `_start` runs in command mode---its `fd_write` journal goes to standard output and the process adopts the guest's `proc_exit` status. Otherwise the first exported function is selected and its return values are printed.
- `--invoke <export>` runs a named export; its arguments follow on the command line as signed or unsigned decimals or `0x` hex, coerced to each parameter's `i32` / `i64` width. A wrong argument count is rejected with a typed diagnostic.
- `--verbose` adds an executed-step and timing summary on standard error.

A bare `ananse` prints a version banner and usage.

## Quick Start

```sh
cargo install ananse

# Recursive Fibonacci: the auto-selected export runs with one argument.
ananse run examples/fibonacci.wat 10                 # -> 89

# A named export with two arguments.
ananse run examples/gcd.wat --invoke gcd 1071 462    # -> 21

# CRC-32 over the "123456789" check vector bundled in the module.
ananse run examples/crc32.wat --invoke crc32 0 9     # -> 3421780262
```

Using the library facade directly:

```rust
use ananse::prelude::*;

let wasm = wat::parse_str(
    r#"(module (func (export "answer") (result i32) (i32.const 42)))"#,
)?;
let mut runtime = Runtime::instantiate(&wasm)?;
let execution = runtime.call(&Entry::Export("answer".into()), &[])?;
assert_eq!(execution.returns, vec![Word::I32(42)]);
```

## API Docs

[docs.rs/ananse](https://docs.rs/ananse/latest/ananse/)

## Repository

[github.com/maatlabs/ananse](https://github.com/maatlabs/ananse). See the [project README](https://github.com/maatlabs/ananse/blob/main/README.md) for the architecture and the full crate layout.

# ananse_decoder

WebAssembly (WASM) module decoder and validator for the Ananse zkVM.

## Role

`ananse_decoder` is Ananse's frontend. `Module::decode` drives `wasmparser`'s validator under a restricted feature profile: only the integer subset of WebAssembly-MVP plus mutable globals is accepted, and floating-point types, SIMD, threads, reference types, GC, tail calls, and exception handling are rejected structurally at validation. Imports are held to a reject-by-default allowlist---only the two deterministic `wasi_snapshot_preview1` functions Ananse can prove (`fd_write`, `proc_exit`) pass. There are no nondeterministic syscalls; the decoder enforces this before a module ever reaches the executor. A decoded `Module` carries the validated bytes plus the extracted import and export metadata every downstream crate consumes.

## Usage

```rust
use ananse_decoder::Module;

let wasm = wat::parse_str(
    r#"(module (func (export "answer") (result i32) (i32.const 42)))"#,
)
.unwrap();

let module = Module::decode(&wasm).expect("valid integer-subset module");
assert!(module.exports().iter().any(|e| e.name == "answer"));
```

## API Docs

[docs.rs/ananse_decoder](https://docs.rs/ananse_decoder/latest/ananse_decoder/)

## Repository

[github.com/maatlabs/ananse](https://github.com/maatlabs/ananse). See the [project README](https://github.com/maatlabs/ananse/blob/main/README.md) for an overview of the full zkVM architecture.

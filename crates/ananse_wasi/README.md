# ananse_wasi

Deterministic minimal WASI (WebAssembly System Interface) host for the Ananse zkVM.

## Role

`ananse_wasi` provides `WasiSnapshotPreview1`, a [`Host`](https://docs.rs/ananse_executor) implementation over the executor's import seam covering exactly the two WASI functions whose behaviour is a pure function of the guest's own linear memory: `fd_write` and `proc_exit`. Unlike a native runtime it never touches an operating-system file descriptor. `fd_write` gathers the `ciovec` array the guest points at and appends the bytes to an in-memory journal the caller reads after the run; `proc_exit` halts the program with its status code. Every memory access is bounds- and arity-checked, so a malformed module is rejected as a host error. The result is a WASI surface that keeps an execution's record stream a deterministic function of `(module, entry, arguments)` alone. Every other WASI import is rejected by `ananse_decoder` at validation time.

## Usage

```rust
use ananse_decoder::Module;
use ananse_executor::{Entry, execute};
use ananse_wasi::WasiSnapshotPreview1;

let module = Module::decode(&hello_world_wasm).expect("decode");
let mut host = WasiSnapshotPreview1::new();
execute(&module, &Entry::Auto, &[], &mut host, &mut ()).expect("run");

assert_eq!(host.journal(), b"Hello, World!\n");
```

## API Docs

[docs.rs/ananse_wasi](https://docs.rs/ananse_wasi/latest/ananse_wasi/)

## Repository

[github.com/maatlabs/ananse](https://github.com/maatlabs/ananse). See the [project README](https://github.com/maatlabs/ananse/blob/main/README.md) for an overview of the full zkVM architecture.

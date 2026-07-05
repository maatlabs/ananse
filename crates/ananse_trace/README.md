# ananse_trace

Register-column execution trace and linear-memory access log for the Ananse zkVM.

## Role

`ananse_trace` is the boundary between execution and proving. `Trace::build` turns the `StepRecord` stream `ananse_executor` emits into the column-major field matrix the STARK prover commits to. One executed operator is one row, 1:1 with the source bytecode. The operand stack, locals, globals, and linear memory lift into one flat address space---linear memory in `[0, 2^32)`, the register file above---and each row records the accesses its operator made on a fixed-width value bus in execution order, alongside the same accesses sorted by address then time. A single argument (the sorted view is a multiset permutation of the bus and is internally read-consistent) replaces the four separate permutations a stack machine would pay for its operand stack, locals, globals, and memory. `layout` fixes the column assignment; `selector` the per-operator selector encoding.

## Usage

```rust
use ananse_decoder::Module;
use ananse_executor::{Entry, execute, global_initializers};
use ananse_lift::lift;
use ananse_trace::Trace;
use ananse_wasi::WasiSnapshotPreview1;

let module = Module::decode(&wasm).expect("decode");
let program = lift(&module).expect("lift");
let globals = global_initializers(&module).expect("globals");

let mut host = WasiSnapshotPreview1::new();
let mut records = Vec::new();
execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("run");

let trace = Trace::build(&program, records, &[], &globals).expect("trace");
assert!(trace.length().is_power_of_two());
```

## API Docs

[docs.rs/ananse_trace](https://docs.rs/ananse_trace/latest/ananse_trace/)

## Repository

[github.com/maatlabs/ananse](https://github.com/maatlabs/ananse). See the [project README](https://github.com/maatlabs/ananse/blob/main/README.md) for an overview of the full zkVM architecture.

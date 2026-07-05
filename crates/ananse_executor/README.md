# ananse_executor

Schedule-driven interpreter and reference semantics for the Ananse zkVM.

## Role

`ananse_executor` runs a validated module under the static register schedule `ananse_lift` produces and emits, per executed operator, the `StepRecord` the trace and AIR consume. It reads opcode semantics and immediates from the module while taking every register address and control-flow successor from the lift schedule, and it cross-checks the operand-stack height it reaches against the schedule's prediction at every step---the live proof that the register lift matches execution, surfaced as `ExecuteError::ScheduleMismatch` on any disagreement. With no external engine, the interpreter *is* Ananse's reference semantics for the integer subset. The record stream is a deterministic function of `(module, entry, arguments, host)`: no wall-clock, randomness, or thread identity, and nondeterministic imports are already rejected at decode time. Values are carried as `Word` (`I32` / `I64`), the unsigned two-limb Goldilocks encoding; imports dispatch through the `Host` trait.

## Usage

```rust
use ananse_executor::{Entry, NoHost, Word, execute};
use ananse_decoder::Module;

let module = Module::decode(&fibonacci_wasm).expect("decode");
let execution = execute(
    &module,
    &Entry::Export("fib".into()),
    &[Word::I32(10)],
    &mut NoHost,       // a module that imports nothing
    &mut (),           // the no-op step observer
)
.expect("run");

assert_eq!(execution.returns, vec![Word::I32(89)]);
```

## API Docs

[docs.rs/ananse_executor](https://docs.rs/ananse_executor/latest/ananse_executor/)

## Repository

[github.com/maatlabs/ananse](https://github.com/maatlabs/ananse). See the [project README](https://github.com/maatlabs/ananse/blob/main/README.md) for an overview of the full zkVM architecture.

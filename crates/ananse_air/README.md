# ananse_air

Register-shaped AIR (algebraic intermediate representation) for the Ananse zkVM.

## Role

`ananse_air` defines `AnanseAir`, the single [`Air`](https://docs.rs/p3-air) the STARK proving path evaluates against the trace `ananse_trace` produces. Its constraints encode the whole register machine over the Goldilocks field. The FRI prover that produces and checks proofs against this AIR ships in a subsequent release; here the constraints are evaluated row by row.

## Usage

```rust
use ananse_air::{AnanseAir, program_data, program_rom};

// `opcodes` and `constants` come from `ananse_executor::{function_opcodes,
// function_constants}`; `function` is the matching `ananse_lift::LiftedFunction`.
let rom = program_rom(&opcodes, function).expect("program ROM");
let data = program_data(&constants, function).expect("program data");

let air = AnanseAir::new(
    &rom,
    &data,
    trace.length(),
    trace.stack_base(),
    trace.initial_state(),
);
```

## API Docs

[docs.rs/ananse_air](https://docs.rs/ananse_air/latest/ananse_air/)

## Repository

[github.com/maatlabs/ananse](https://github.com/maatlabs/ananse). See the [project README](https://github.com/maatlabs/ananse/blob/main/README.md) for an overview of the full zkVM architecture.

# ananse_lift

Static stack-to-register lift for the Ananse zkVM.

## Role

`ananse_lift` is Ananse's architectural centerpiece. `lift` turns a validated `Module` into a `LiftedProgram` that records, for every program point, the operand-stack height, the depth-indexed register operands each instruction reads and writes, the per-function register-file width, and the control-flow successor map. WebAssembly validation already fixes a single operand-stack height at every reachable program point; the lift re-derives it by abstract interpretation over the structured control flow and addresses the operand stack, locals, and globals as one positional register file (`Register`). This is what makes a register-shaped proof of WebAssembly possible without compiling WASM away: register identity is static, so a proof's addresses are fixed at analysis time rather than tracked by a runtime stack pointer. Because every predecessor of a control-flow join agrees on height, merges need no value muxing---only the next program point is data-dependent, surfaced through `Successors`.

## Usage

```rust
use ananse_decoder::Module;
use ananse_lift::lift;

let module = Module::decode(&wasm).expect("decode");
let program = lift(&module).expect("lift");

for function in &program.functions {
    for instr in &function.instrs {
        // `instr.height_in` is the operand-stack height on entry to this program
        // point; `instr.reads` / `instr.writes` are its register operands.
        let _ = (instr.pc, instr.height_in, &instr.reads, &instr.writes);
    }
}
```

## API Docs

[docs.rs/ananse_lift](https://docs.rs/ananse_lift/latest/ananse_lift/)

## Repository

[github.com/maatlabs/ananse](https://github.com/maatlabs/ananse). See the [project README](https://github.com/maatlabs/ananse/blob/main/README.md) for an overview of the full zkVM architecture.

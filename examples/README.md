# Examples

Production-grade programs that ship with Ananse. Every file here is a real integer workload the zkVM should prove/verify end-to-end---a recurrence, a number-theory kernel, a checksum, a hash round, a Merkle-path fold---written in the integer subset of WebAssembly-MVP and runnable end to end through the `ananse run` command. This release executes and journals; it does not yet produce proofs. Each is also asserted against an independent reference in the test suite (`tests/tests/execute.rs::examples_compute_expected_values`).

## Prerequisites

Install the CLI per the [root README](../README.md#installation), or substitute `cargo run --release --` for `ananse` in every command below when working from a source build (for example, `cargo run --release -- run examples/fibonacci.wat 10`).

## How to run an example

`ananse run` decodes, lifts, and executes a module, printing its result (and any WASI journal):

```bash
ananse run examples/<name>.wat [--invoke <export>] [args...]
```

With no `--invoke`, the first exported function is selected. Arguments follow on the command line as signed or unsigned decimals or `0x` hex, coerced to each parameter's `i32` / `i64` width; a wrong argument count is rejected with a typed diagnostic. Add `--verbose` for an executed-step and timing summary on standard error.

## Programs

| Program           | What it does                                         | Example invocation                                               | Result                      |
| ----------------- | ---------------------------------------------------- | ---------------------------------------------------------------- | --------------------------- |
| `fibonacci.wat`   | Recursive Fibonacci `fib(n)`                         | `ananse run examples/fibonacci.wat 10`                           | `89`                        |
| `factorial.wat`   | Iterative `n!` over 64-bit integers                  | `ananse run examples/factorial.wat --invoke factorial 10`        | `3628800`                   |
| `gcd.wat`         | Greatest common divisor (Euclid)                     | `ananse run examples/gcd.wat --invoke gcd 1071 462`              | `21`                        |
| `fnv1a.wat`       | FNV-1a 64-bit hash over bytes in linear memory       | `ananse run examples/fnv1a.wat --invoke fnv1a 0 6`               | `8582844739662639449`       |
| `modpow.wat`      | Modular exponentiation `base^exp mod m`              | `ananse run examples/modpow.wat --invoke modpow 4 13 497`        | `445`                       |
| `crc32.wat`       | CRC-32 (IEEE 802.3, reflected) checksum              | `ananse run examples/crc32.wat --invoke crc32 0 9`               | `3421780262` (`0xCBF43926`) |
| `merkle_path.wat` | Merkle authentication-path fold (FNV-1a compression) | `ananse run examples/merkle_path.wat --invoke merkle_root 1 0 3` | `3251291996388540232`       |

`fnv1a`, `crc32`, and `merkle_path` read their input bytes from a data segment bundled in the module (the six bytes of "Ananse", the "123456789" check vector, and three sibling hashes respectively), so the pointer/length or depth arguments index into that bundled data. `crc32` over "123456789" reproduces the standard CRC-32/ISO-HDLC check value `0xCBF43926`.

## Notes

The proving path (a STARK `prove` / `verify` over these same programs) arrives with the prover in a later release; the run commands above are stable and will gain a proof step then.

# Security Policy & Threat Model

This document describes the trust boundaries, attacker model, and mitigations for the Ananse zkVM. It covers the current state as of v0.2.0 and will be updated as subsequent versions introduce new attack surfaces---most notably the STARK prover and verifier, which do not exist in this release.

Ananse is a research project. It has not been audited and is not ready for production use.

## Trust Boundaries

v0.2.0 is the executable virtual machine. A module is untrusted input; the pipeline that runs it is trusted code:

```text
Module (.wasm/.wat) --> Decode & Validate --> Lift --> Execute --> Result + WASI journal
        │                     │                 │          │                 │
     Untrusted            Trusted           Trusted     Trusted           Observed
   (user input)         (our code)        (our code)  (our code)      (program output)
```

1. **Module boundary.** `.wasm` / `.wat` modules are untrusted input. The decoder must reject arbitrary, malformed, or adversarial modules before execution, and the executor must validate every operand it consumes.

2. **Execution boundary.** Even a well-formed module may attempt resource exhaustion (unbounded recursion, memory growth, out-of-bounds access, etc.) or try to escape determinism through a host import. The executor enforces runtime limits and the host surface is deterministic by construction.

The **proof boundary**---an untrusted `.proof` arriving over an adversarial channel---does not exist in this release. The FRI STARK prover and verifier land in a later version, and this document will grow an "adversarial proof" attacker class when they do. The determinism guarantees below are the foundation that soundness argument will rest on.

## Attacker Model

Each attacker class carries a **status**, so this document reads as a living ledger instead of a flat list of claims:

- **Mitigated@\<release\>** --- an active check, limit, or constraint defends the vector, guarded by a test. The stamp records when the defense first shipped.
- **Accepted-risk** --- the vector is understood and consciously left undefended because the current execution model (single-user, local CLI, no deployed verifiers) puts it out of scope. Revisited if that model changes.
- **Not-yet-present** --- the surface does not exist in this release; the defense ships with the feature that introduces it.

### Attacker 1: Malicious WebAssembly module

**Goal:** crash the decoder or executor, exhaust CPU or memory, smuggle in nondeterminism, or corrupt memory through a crafted module.

**Status:** Mitigated@v0.2.0. Every vector below has an active check, exercised by the decoder, lift, and executor test suites.

| Attack vector                                                                         | Mitigation                                                                                                              | Location                          |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | --------------------------------- |
| Non-integer proposals (floats, SIMD, threads, GC, references, tail calls, exceptions) | `wasmparser` validator under a restricted `WasmFeatures` profile; rejected as `DecodeError::ValidationFailed`           | `ananse_decoder`                  |
| Nondeterministic host imports (clocks, entropy, sockets, filesystem)                  | Reject-by-default WASI allowlist---only `fd_write` / `proc_exit` pass (`ForbiddenImportModule` / `ForbiddenWasiImport`) | `ananse_decoder`                  |
| Malformed or truncated binary                                                         | Validated before execution; `DecodeError::InvalidBinary` / `ValidationFailed`                                           | `ananse_decoder`                  |
| Deeply nested expressions inflating the register file                                 | Register-file width cap (`MAX_REGISTER_FILE_WIDTH = 4096`), reject-on-exceed via `LiftError`                            | `ananse_lift`                     |
| Out-of-bounds operand indices (locals, globals, functions)                            | Resolved and bounds-checked during the static lift and execution                                                        | `ananse_lift` / `ananse_executor` |
| Unbounded recursion                                                                   | Call-depth cap (`MAX_CALL_DEPTH = 1024`) → `Trap::CallStackExhausted`                                                   | `ananse_executor`                 |
| Linear-memory growth abuse                                                            | Growth bounded by the wasm32 page ceiling and the module's declared maximum                                             | `ananse_executor`                 |
| Out-of-bounds load / store                                                            | Effective address bounds-checked → `Trap::MemoryOutOfBounds`                                                            | `ananse_executor`                 |
| Integer divide-by-zero and `MIN / -1` overflow                                        | `checked_div` / `checked_rem` plus an explicit overflow check → `Trap::DivideByZero` / `Trap::IntegerOverflow`          | `ananse_executor`                 |
| Narrowing integer casts                                                               | Every cast goes through `TryFrom` with a range-checked error                                                            | workspace-wide                    |
| Malformed WASI arguments (bad iovec, wrong arity)                                     | Every host memory access is bounds- and arity-checked; surfaced as `ExecuteError::Host`, never a panic                  | `ananse_wasi`                     |
| Static schedule disagreeing with execution                                            | Per-program-point operand-stack-height cross-check → `ExecuteError::ScheduleMismatch`                                   | `ananse_executor`                 |
| Memory corruption via `unsafe`                                                        | `#![forbid(unsafe_code)]` in every crate; the workspace contains zero `unsafe` blocks                                   | workspace-wide                    |

### Attacker 2: Resource exhaustion

**Goal:** make the executor consume unbounded CPU time or memory.

**Status:** Mitigated@v0.2.0 for recursion, register-file, and memory growth; unbounded loop time is Accepted-risk under the single-user CLI model.

| Resource            | Limit                                | Enforcement               |
| ------------------- | ------------------------------------ | ------------------------- |
| Call frames         | 1024                                 | `MAX_CALL_DEPTH`          |
| Register-file width | 4096                                 | `MAX_REGISTER_FILE_WIDTH` |
| Linear memory       | wasm32 page ceiling + module maximum | page-growth check         |

**Accepted-risk:** a well-formed module with a non-terminating loop (bounded memory, no recursion) runs until the process is interrupted---there is no instruction or gas budget. This is acceptable for a local, single-user CLI; a step budget lands if a multi-tenant host or a proving-cost bound ever makes it load-bearing.

### Attacker 3: Adversarial proof

**Status:** Not-yet-present. Ananse produces no proofs in v0.2.0, so there is no verifier to attack. When the FRI STARK prover and verifier ship, this class will document wrong-program substitution, tampered-trace, and forged-public-input vectors against the register-shaped AIR's constraint system.

## Memory Safety

All workspace member crates enforce `#![forbid(unsafe_code)]`. The codebase contains zero `unsafe` blocks; memory safety is guaranteed by the Rust type system and borrow checker.

## Arithmetic Safety

All integer arithmetic in Ananse's own bookkeeping uses Rust's `checked_*` methods, and every integer cast goes through `TryFrom` with a range-checked error---no silent wrapping or truncation. The WebAssembly operators themselves follow the spec's semantics: defined wrapping for `add` / `sub` / `mul`, and traps for divide-by-zero and the `INT_MIN / -1` overflow.

## Determinism

Ananse's execution is deterministic by construction: the emitted record stream is a pure function of `(module, entry, arguments, host)`. No wall-clock time, thread identity, or randomness enters the execution path, and every nondeterministic WASI import is rejected at decode time. This is a security property; it's not merely a convenience---it makes every run reproducible today, and it is the foundation the STARK soundness argument will rest on once the prover is shipped.

## Reporting Vulnerabilities

**Please do not open a GitHub issue or pull request to report a security vulnerability.** This makes the problem immediately visible to everyone, including malicious actors.

Report privately via [GitHub Security Advisories](https://github.com/maatlabs/ananse/security/advisories/new).

### Coordinated disclosure

We commit to:

- **Acknowledging your report within 7 days** of receipt.
- **Releasing a fix within 90 days**, or coordinating an extension with you in writing if the issue is unusually involved.

If 90 days pass without a fix and without a written extension, you are free to disclose publicly. We ask only that you give us a final 7-day notice before doing so, so we can prepare downstream users.

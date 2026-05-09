use alloc::string::String;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("invalid WASM binary at offset {offset}: {message}")]
    InvalidBinary { offset: usize, message: String },

    #[error("WASM module failed validation at offset {offset}: {message}")]
    ValidationFailed { offset: usize, message: String },

    #[error(
        "floating-point operations are currently disabled (function {func_idx}, instruction {instr_idx}: {opcode})"
    )]
    FloatsDisabled {
        func_idx: u32,
        instr_idx: u32,
        opcode: &'static str,
    },

    #[error("import module `{module}` is not permitted (only `wasi_snapshot_preview1` is allowed)")]
    ForbiddenImportModule { module: String },

    #[error(
        "WASI import `{module}::{name}` is not permitted (only `fd_write` and `proc_exit` are allowed)"
    )]
    ForbiddenWasiImport { module: String, name: String },
}

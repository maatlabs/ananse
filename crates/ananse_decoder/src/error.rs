use alloc::string::String;

use wasmparser::BinaryReaderError;

pub(crate) fn invalid_binary(e: BinaryReaderError) -> DecodeError {
    DecodeError::InvalidBinary {
        offset: e.offset(),
        message: e.to_string(),
    }
}

pub(crate) fn validation_failed(e: BinaryReaderError) -> DecodeError {
    DecodeError::ValidationFailed {
        offset: e.offset(),
        message: e.to_string(),
    }
}

/// An error produced while decoding or validating a WebAssembly module.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The byte stream is not a well-formed WebAssembly binary.
    #[error("invalid WASM binary at offset {offset}: {message}")]
    InvalidBinary {
        /// Byte offset at which the malformed input was detected.
        offset: usize,
        /// Human-readable detail from the underlying parser.
        message: String,
    },

    /// The module is well-formed but fails validation under Ananse's restricted
    /// feature set. Floating-point types and instructions, SIMD, threads, GC,
    /// reference types, multi-memory, tail calls, and exceptions are all
    /// rejected here, because their corresponding features are left disabled.
    #[error("WASM module failed validation at offset {offset}: {message}")]
    ValidationFailed {
        /// Byte offset at which validation failed.
        offset: usize,
        /// Human-readable detail from the validator.
        message: String,
    },

    /// An import references a module other than `wasi_snapshot_preview1`.
    #[error("import module `{module}` is not permitted (only `wasi_snapshot_preview1` is allowed)")]
    ForbiddenImportModule {
        /// The rejected import module namespace.
        module: String,
    },

    /// An import references a WASI function outside the deterministic set Ananse
    /// supports (`fd_write`, `proc_exit`).
    #[error(
        "WASI import `{module}::{name}` is not permitted (only `fd_write` and `proc_exit` are allowed)"
    )]
    ForbiddenWasiImport {
        /// The import module namespace (always `wasi_snapshot_preview1` here).
        module: String,
        /// The rejected WASI function name.
        name: String,
    },
}

use alloc::string::String;

use wasmparser::BinaryReaderError;

/// An error produced while decoding or validating a WASM module.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The byte stream is not a well-formed WASM binary.
    #[error("invalid WASM binary at offset {offset}: {message}")]
    InvalidBinary {
        /// The byte offset at which the malformed input was detected.
        offset: usize,
        /// The underlying parser error message.
        message: String,
    },

    /// The module is well-formed but fails validation under the allowed
    /// WASM feature set.
    #[error("WASM module failed validation at offset {offset}: {message}")]
    ValidationFailed {
        /// The byte offset at which validation failed.
        offset: usize,
        /// The underlying validation error message.
        message: String,
    },

    /// An operator not found in [wasmparser::Operator] enum was reached.
    #[error("unsupported operator at offset {offset}: {operator}")]
    UnsupportedOperator {
        /// Byte offset of the rejected operator.
        offset: usize,
        /// Which operator was rejected.
        operator: String,
    },

    /// An import references a WASM module other than `wasi_snapshot_preview1`.
    #[error("import module `{module}` is not permitted (only `wasi_snapshot_preview1` is allowed)")]
    ForbiddenImportModule {
        /// The rejected import module namespace.
        module: String,
    },

    /// An import references a WASI function outside the supported set.
    #[error(
        "WASI import `{module}::{name}` is not permitted (only `fd_write` and `proc_exit` are allowed)"
    )]
    ForbiddenWasiImport {
        /// The rejected import module namespace.
        module: String,
        /// The rejected WASI function name.
        name: String,
    },
}

impl DecodeError {
    /// Reports that the byte stream is not a well-formed WASM binary.
    pub fn invalid_binary(e: BinaryReaderError) -> Self {
        Self::InvalidBinary {
            offset: e.offset(),
            message: e.to_string(),
        }
    }

    /// Reports that the module is well-formed but fails validation under the
    /// allowed WASM feature set.
    pub fn validation_failed(e: BinaryReaderError) -> Self {
        Self::ValidationFailed {
            offset: e.offset(),
            message: e.to_string(),
        }
    }
}

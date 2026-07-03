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
    #[error("invalid WASM binary at offset {offset}: {message}")]
    InvalidBinary { offset: usize, message: String },

    #[error("WASM module failed validation at offset {offset}: {message}")]
    ValidationFailed { offset: usize, message: String },

    #[error("import module `{module}` is not permitted (only `wasi_snapshot_preview1` is allowed)")]
    ForbiddenImportModule { module: String },

    #[error(
        "WASI import `{module}::{name}` is not permitted (only `fd_write` and `proc_exit` are allowed)"
    )]
    ForbiddenWasiImport { module: String, name: String },
}

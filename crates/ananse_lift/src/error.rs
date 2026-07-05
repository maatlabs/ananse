use alloc::string::String;

use wasmparser::BinaryReaderError;

pub(crate) fn internal(message: &str) -> LiftError {
    LiftError::MalformedModule {
        offset: 0,
        message: message.to_string(),
    }
}

pub(crate) fn malformed(e: BinaryReaderError) -> LiftError {
    LiftError::MalformedModule {
        offset: e.offset(),
        message: e.to_string(),
    }
}

/// An error produced while lifting a validated module to the static register form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiftError {
    #[error("malformed module at offset {offset}: {message}")]
    MalformedModule { offset: usize, message: String },

    #[error("unsupported operator at offset {offset}")]
    UnsupportedOperator { offset: usize },

    #[error("function {func_index} register-file width {width} exceeds the supported maximum")]
    RegisterFileOverflow { func_index: u32, width: u64 },

    #[error("function {func_index} has too many operators to lift")]
    FunctionTooLarge { func_index: u32 },
}

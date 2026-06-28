use alloc::string::String;

use wasmparser::BinaryReaderError;

/// Builds an internal-inconsistency error. A validated module never triggers
/// these paths; they keep the lift total instead of panicking.
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
///
/// A [`Module`](ananse_decoder::Module) reaches the lift only after the decoder has
/// validated it, so [`LiftError::MalformedModule`] should never occur in
/// practice; it exists to keep the lift total rather than panicking on an
/// internally inconsistent input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiftError {
    /// The module's bytes could not be parsed. Indicates an inconsistency
    /// between the decoder's validation and the lift's re-parse.
    #[error("malformed module at offset {offset}: {message}")]
    MalformedModule {
        /// Byte offset at which parsing failed.
        offset: usize,
        /// Human-readable detail from the underlying parser.
        message: String,
    },

    /// An operator outside Ananse's integer subset of WASM was encountered.
    #[error("unsupported operator at offset {offset}")]
    UnsupportedOperator {
        /// Byte offset of the rejected operator.
        offset: usize,
    },

    /// A function's register-file width (locals + globals + maximum operand-stack
    /// height) exceeds the supported maximum, or the operand stack grows past the
    /// representable range.
    #[error("function {func_index} register-file width {width} exceeds the supported maximum")]
    RegisterFileOverflow {
        /// The offending function's index in the module function index space.
        func_index: u32,
        /// The computed register-file width that breached the limit.
        width: u64,
    },

    /// A function body has more operators than can be indexed as program points.
    #[error("function {func_index} has too many operators to lift")]
    FunctionTooLarge {
        /// The offending function's index in the module function index space.
        func_index: u32,
    },
}

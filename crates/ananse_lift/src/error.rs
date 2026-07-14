use ananse_decoder::DecodeError;
use wasmparser::BinaryReaderError;

/// An error produced while lifting a validated module to the static register form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiftError {
    /// The module's bytes could not be parsed into an executable image.
    #[error("failed to decode module: {0}")]
    Decode(#[from] DecodeError),

    /// An operator outside the allowed integer subset of WASM was encountered.
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

impl LiftError {
    /// Reports that the module's bytes could not be parsed. Indicates an
    /// inconsistency between the decoder's validation and the lift's re-parse.
    pub fn malformed(e: BinaryReaderError) -> Self {
        Self::Decode(DecodeError::InvalidBinary {
            offset: e.offset(),
            message: e.to_string(),
        })
    }

    /// Builds an internal-inconsistency error.
    ///
    /// A validated module never triggers these paths;
    /// they keep the lift total instead of panicking.
    pub(crate) fn internal(message: &str) -> Self {
        Self::Decode(DecodeError::InvalidBinary {
            offset: 0,
            message: message.into(),
        })
    }
}

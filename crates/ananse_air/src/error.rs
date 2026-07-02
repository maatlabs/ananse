//! Errors raised while constructing the AIR.

/// Maps a LogUp witness-build failure to an [`AirError`].
pub fn build_error<E: core::fmt::Display>(error: E) -> AirError {
    AirError::LookupBuild(error.to_string())
}

/// A failure encountered while building AIR-side data from a lifted program.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AirError {
    /// The per-program-point opcode sequence and the lift schedule disagree on
    /// the function body's length; they must describe the same program.
    #[error("opcode sequence ({opcodes}) and schedule ({schedule}) disagree in length")]
    ScheduleLengthMismatch {
        /// Length of the supplied per-program-point opcode sequence.
        opcodes: usize,
        /// Length of the lift schedule.
        schedule: usize,
    },

    /// The function body has more program points than the program ROM can pack a
    /// control-flow edge for while staying injective below the Goldilocks prime.
    #[error("function body of {body_len} program points is too large for the program ROM")]
    ProgramTooLarge {
        /// Number of program points in the offending function body.
        body_len: usize,
    },

    /// The LogUp witness columns could not be built, most often because a trace row
    /// looks up a control-flow edge absent from the program ROM.
    #[error("could not build the control-flow lookup witness: {0}")]
    LookupBuild(String),
}

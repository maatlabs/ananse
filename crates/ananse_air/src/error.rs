//! Errors raised while constructing the AIR.

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

    /// The padded trace is too short to embed the program ROM: the lookup argument
    /// needs one witness row per ROM entry above the leading spacer row.
    #[error("trace of {trace_len} rows is too short to embed a {rom_len}-entry program ROM")]
    TraceTooShortForRom {
        /// Padded height of the trace.
        trace_len: usize,
        /// Number of entries in the program ROM.
        rom_len: usize,
    },

    /// The LogUp folding challenge collided with a folded ROM entry or edge, leaving a
    /// zero denominator. A fresh challenge resolves it; over the quadratic extension
    /// this is negligibly unlikely.
    #[error("the LogUp folding challenge collided with a folded value")]
    DegenerateChallenge,
}

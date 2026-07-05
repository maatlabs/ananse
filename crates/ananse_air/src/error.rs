//! Errors raised while constructing the AIR.

/// A failure encountered while building AIR-side data from a lifted program.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AirError {
    #[error("opcode sequence ({opcodes}) and schedule ({schedule}) disagree in length")]
    ScheduleLengthMismatch {
        /// Length of the supplied per-program-point opcode sequence.
        opcodes: usize,
        /// Length of the lift schedule.
        schedule: usize,
    },

    #[error("function body of {body_len} program points is too large for the program ROM")]
    ProgramTooLarge {
        /// Number of program points in the offending function body.
        body_len: usize,
    },

    #[error("could not build the lookup witness: {0}")]
    LookupBuild(String),

    #[error("trace of {trace_len} rows is too short to embed a {rom_len}-entry program ROM")]
    TraceTooShortForRom {
        /// Padded height of the trace.
        trace_len: usize,
        /// Number of entries in the program ROM.
        rom_len: usize,
    },

    #[error(
        "trace of {trace_len} rows is too short to embed a {table_len}-entry initial-state table"
    )]
    TraceTooShortForBoundary {
        /// Padded height of the trace.
        trace_len: usize,
        /// Number of entries in the initial-state table.
        table_len: usize,
    },

    #[error("the LogUp folding challenge collided with a folded value")]
    DegenerateChallenge,
}

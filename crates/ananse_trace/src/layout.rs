//! Column layout of the register-shaped trace.
//!
//! A trace row is `[Felt; trace_width(W)]` for a register file of width `W`. The
//! columns are laid out in four contiguous groups, low index to high:
//!
//! 1. control and memory columns ([`COL_PC`] through [`COL_MEM_IS_WRITE`]);
//! 2. the one-hot selector block `[SELECTOR_BASE, SELECTOR_BASE + NUM_SELECTORS)`;
//! 3. the depth-indexed register bank `[REGISTER_BASE, REGISTER_BASE + W)`.

use crate::selector::NUM_SELECTORS;

/// Program counter: the operator's index within its function body.
pub const COL_PC: usize = 0;
/// Effective byte address of a linear-memory access; zero on rows with none.
pub const COL_MEM_ADDR: usize = 1;
/// Little-endian value read or written by a linear-memory access, as a field
/// residue; zero on rows with none.
pub const COL_MEM_VAL: usize = 2;
/// One on a store row, zero otherwise (loads and non-memory rows alike).
pub const COL_MEM_IS_WRITE: usize = 3;

/// First selector column. The one-hot selector block spans
/// `[SELECTOR_BASE, SELECTOR_BASE + NUM_SELECTORS)`; exactly one column in it is
/// one on every row.
pub const SELECTOR_BASE: usize = 4;

/// First register column. The register bank spans
/// `[REGISTER_BASE, REGISTER_BASE + W)` for a register file of width `W`.
pub const REGISTER_BASE: usize = SELECTOR_BASE + NUM_SELECTORS;

/// Total column count for a register file of width `register_width`.
pub const fn trace_width(register_width: usize) -> usize {
    REGISTER_BASE + register_width
}

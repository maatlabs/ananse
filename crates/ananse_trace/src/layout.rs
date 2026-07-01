//! Column layout of the register-shaped trace.
//!
//! A trace row is `[Felt; trace_width(W)]` for a register file of width `W`. The
//! columns are laid out in three contiguous groups, low index to high:
//!
//! 1. control and memory columns ([`COL_PC`] through [`COL_MEM_IS_WRITE`]);
//! 2. the one-hot selector block `[SELECTOR_BASE, SELECTOR_BASE + NUM_SELECTORS)`;
//! 3. the depth-indexed register bank `[REGISTER_BASE, REGISTER_BASE + W * REGISTER_LIMBS)`.
//!
//! Every runtime value is carried as two little-endian 32-bit limbs `(lo, hi)`
//! with the value equal to `lo + hi * 2^32`, because a single Goldilocks element
//! cannot injectively hold the `i64` values at or above the prime. Each register
//! slot therefore occupies [`REGISTER_LIMBS`] columns, and the linear-memory
//! value is split across [`COL_MEM_VAL_LO`] and [`COL_MEM_VAL_HI`].

use crate::selector::NUM_SELECTORS;

/// Field limbs per value: an `i64` needs two 32-bit limbs, an `i32` uses the low
/// limb alone (high limb zero).
pub const REGISTER_LIMBS: usize = 2;

/// Program counter: the operator's index within its function body.
pub const COL_PC: usize = 0;
/// Effective byte address of a linear-memory access; zero on rows with none. A
/// valid (non-trapping) access lies within a memory of at most `2^32` bytes.
pub const COL_MEM_ADDR: usize = 1;
/// Low 32-bit limb of the value read or written by a linear-memory access; zero
/// on rows with none.
pub const COL_MEM_VAL_LO: usize = 2;
/// High 32-bit limb of the value read or written by a linear-memory access; zero
/// on rows with none and on any access narrower than 64 bits.
pub const COL_MEM_VAL_HI: usize = 3;
/// One on a store row, zero otherwise (loads and non-memory rows alike).
pub const COL_MEM_IS_WRITE: usize = 4;

/// First selector column. The one-hot selector block spans
/// `[SELECTOR_BASE, SELECTOR_BASE + NUM_SELECTORS)`; exactly one column in it is
/// one on every row.
pub const SELECTOR_BASE: usize = 5;

/// First register column. The register bank spans
/// `[REGISTER_BASE, REGISTER_BASE + W * REGISTER_LIMBS)` for a register file of
/// width `W`, each slot holding its `(lo, hi)` limbs in adjacent columns.
pub const REGISTER_BASE: usize = SELECTOR_BASE + NUM_SELECTORS;

/// The `(lo, hi)` limb columns of the register-bank slot at `offset`.
pub const fn register_limb_columns(offset: usize) -> (usize, usize) {
    let lo = REGISTER_BASE + offset * REGISTER_LIMBS;
    (lo, lo + 1)
}

/// Total column count for a register file of width `register_width`.
pub const fn trace_width(register_width: usize) -> usize {
    REGISTER_BASE + register_width * REGISTER_LIMBS
}

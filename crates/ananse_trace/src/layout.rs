//! Column layout of the register-shaped trace.
//!
//! Ananse lifts WebAssembly's operand stack, locals, globals, and linear memory
//! into a single flat address space and proves every access consistent through one
//! argument: an address-sorted access log permuted against the execution-order
//! accesses. A trace row is therefore not a snapshot of a register file but a
//! record of the accesses one operator performed, carried on a fixed-width *value
//! bus*, plus the sorted-log view the permutation runs over.
//!
//! The main-trace columns are laid out in five contiguous groups, low index to
//! high:
//!
//! 1. control columns ([`COL_PC`], [`COL_CLK`], [`COL_FRAME_BASE`], [`COL_HEIGHT`],
//!    [`COL_IMM`]);
//! 2. the execution-order value bus: [`BUS_SLOTS`] access slots, each
//!    `(addr, lo, hi, is_write, active)` ([`BUS_BASE`]);
//! 3. the address-sorted access log: [`BUS_SLOTS`] sorted entries per row, each
//!    `(addr, ts, lo, hi, is_write, active)`, plus the per-entry sortedness witness
//!    ([`SORTED_BASE`]);
//! 4. the one-hot opcode selector block ([`SELECTOR_BASE`]);
//! 5. the per-opcode arithmetic witness block ([`witness_base`]).
//!
//! Every runtime value is carried as two little-endian 32-bit limbs `(lo, hi)` with
//! the value equal to `lo + hi * 2^32`, because a single Goldilocks element cannot
//! injectively hold the `i64` values at or above the prime. The address space keeps
//! linear memory in the low `2^32` byte range and the register file above it, so a
//! register address never collides with a memory address and the one permutation
//! covers both.

use crate::selector::NUM_SELECTORS;

/// Field limbs per value: an `i64` needs two 32-bit limbs, an `i32` uses the low
/// limb alone (high limb zero).
pub const REGISTER_LIMBS: usize = 2;

/// Access slots on the value bus, and equally the number of address-sorted log
/// entries laid down per row.
pub const BUS_SLOTS: usize = 4;

/// First byte address of the register file.
pub const REGISTER_REGION: u64 = 1 << 32;

/// Program counter: the operator's index within its function body.
pub const COL_PC: usize = 0;
/// Row clock: the zero-based row index.
pub const COL_CLK: usize = 1;
/// Base address of the executing activation's register frame. Zero for a single call frame.
pub const COL_FRAME_BASE: usize = 2;
/// Operand-stack height entering the operator.
pub const COL_HEIGHT: usize = 3;
/// Register-file offset of the local or global slot the operator touches, or zero
/// for operators that touch none.
pub const COL_IMM: usize = 4;

/// First column of the execution-order value bus.
pub const BUS_BASE: usize = 5;
/// Columns per value-bus slot: the access address, its value's two limbs, the
/// store flag, and the active flag.
pub const BUS_SLOT_COLS: usize = 5;

/// Column offsets within one value-bus slot, from the slot's base column.
pub mod slot {
    /// Access address in the unified address space.
    pub const ADDR: usize = 0;
    /// Low 32-bit limb of the accessed value.
    pub const LO: usize = 1;
    /// High 32-bit limb of the accessed value.
    pub const HI: usize = 2;
    /// One on a write (store) access, zero on a read (load).
    pub const IS_WRITE: usize = 3;
    /// One when the slot carries a real access, zero when it is idle.
    pub const ACTIVE: usize = 4;
}

/// First column of the value-bus slot at index `slot` (`slot < BUS_SLOTS`).
pub const fn bus_slot(slot: usize) -> usize {
    BUS_BASE + slot * BUS_SLOT_COLS
}

/// First column of the address-sorted access log.
pub const SORTED_BASE: usize = BUS_BASE + BUS_SLOTS * BUS_SLOT_COLS;
/// Columns per sorted-log entry: the address, the originating timestamp, the two
/// value limbs, the store flag, the active flag, and the sortedness witness.
pub const SORTED_SLOT_COLS: usize = 7;

/// Column offsets within one sorted-log entry, from the entry's base column.
pub mod sorted {
    /// Access address; entries are laid in non-decreasing address order.
    pub const ADDR: usize = 0;
    /// Timestamp the access carried on the value bus, `r * BUS_SLOTS + s`.
    pub const TS: usize = 1;
    /// Low 32-bit limb of the accessed value.
    pub const LO: usize = 2;
    /// High 32-bit limb of the accessed value.
    pub const HI: usize = 3;
    /// One on a write access, zero on a read.
    pub const IS_WRITE: usize = 4;
    /// One when the entry is real, zero when it pads the log's tail.
    pub const ACTIVE: usize = 5;
    /// One when this entry shares the previous entry's address (a continuing access
    /// to the same cell), zero when it opens a new address.
    pub const SAME_ADDR: usize = 6;
}

/// First column of the sorted-log entry at index `slot` (`slot < BUS_SLOTS`).
pub const fn sorted_slot(slot: usize) -> usize {
    SORTED_BASE + slot * SORTED_SLOT_COLS
}

/// First selector column. The one-hot selector block spans
/// `[SELECTOR_BASE, SELECTOR_BASE + NUM_SELECTORS)`; exactly one column in it is one
/// on every row.
pub const SELECTOR_BASE: usize = SORTED_BASE + BUS_SLOTS * SORTED_SLOT_COLS;

/// First column of the per-opcode arithmetic witness block, immediately after the
/// selectors. Multiplication and division carry auxiliary product and
/// quotient/remainder limbs here; families that need no witness ignore it.
pub const fn witness_base() -> usize {
    SELECTOR_BASE + NUM_SELECTORS
}

/// Total number of main-trace columns: the control columns, the value bus, the
/// sorted log, the selectors, and the [`WITNESS_COLS`] arithmetic-witness columns.
pub const fn main_width() -> usize {
    witness_base() + WITNESS_COLS
}

/// Arithmetic-witness columns shared by the value-bus opcode families. The widest
/// consumer is 64-bit multiplication, whose four 32-bit partial-product limbs and
/// two carry limbs pin the full 128-bit product before it is truncated.
pub const WITNESS_COLS: usize = 6;

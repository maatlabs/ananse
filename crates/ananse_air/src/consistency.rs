//! Read-consistency over the address-sorted access log.
//!
//! The sorted log lays every access in non-decreasing `(address, timestamp)` order,
//! [`BUS_SLOTS`] entries per row. Reading it as one flat sequence, each entry is
//! constrained against the one before it: the active entries form a prefix (no real
//! access hides behind a padding entry), entries that share an address agree on it,
//! and a read of a continuing address returns the value the previous access left.
//! Together with the permutation tying this log to the execution-order bus, that is
//! what makes every operand-stack, local, global, and memory read return the value
//! last written to its cell.
//!
//! Consecutive entries are checked pairwise. Within a row the pairs are adjacent
//! slots; across a row boundary the pair is the current row's last entry and the
//! next row's first. Evaluating both on each row transition covers every consecutive
//! pair in the flat sequence exactly once. The pairs cannot be summed into one
//! residual---no one-hot selector isolates a single pair---so each pair contributes
//! its own residuals.
//!
//! The strict orderings that distinguish "same cell, later time" from "a greater
//! address" are range checks on the gaps and are enforced by the range-check
//! argument, not here; this module fixes the equalities those orderings sit on.

use ananse_trace::layout::BUS_SLOTS;
use maat_field::{Felt, FieldElement};
use winter_air::TransitionConstraintDegree;

use crate::bus::SortedEntry;

/// Residuals per consecutive-entry pair: the active-prefix absorber, the shared-
/// address equality, and the two read-consistency limb checks.
const PER_PAIR: usize = 4;

/// Number of consistency constraints: [`PER_PAIR`] for each of the [`BUS_SLOTS`]
/// consecutive pairs a row transition spans.
pub(crate) const NUM_CONSTRAINTS: usize = PER_PAIR * BUS_SLOTS;

/// Algebraic degree of each consistency residual: the active absorber `(1 - a) * a'`
/// is degree two, the shared-address equality is degree three, and each read-value
/// check gates on activity, continuation, and read-ness before the limb difference,
/// degree four.
pub(crate) fn degrees() -> Vec<TransitionConstraintDegree> {
    (0..BUS_SLOTS)
        .flat_map(|_| [2, 3, 4, 4])
        .map(TransitionConstraintDegree::new)
        .collect()
}

/// Evaluates the read-consistency residuals on the `current`/`next` row pair. The
/// consecutive pairs are the adjacent sorted slots within `current` and the wrap
/// from `current`'s last slot to `next`'s first.
pub(crate) fn evaluate<E: FieldElement<BaseField = Felt>>(
    current: &[E],
    next: &[E],
    result: &mut [E],
) {
    for pair in 0..BUS_SLOTS {
        let prev = SortedEntry::read(current, pair);
        let cur = if pair + 1 < BUS_SLOTS {
            SortedEntry::read(current, pair + 1)
        } else {
            SortedEntry::read(next, 0)
        };
        let base = pair * PER_PAIR;
        // No real access may follow a padding entry, so the active flags are a
        // non-increasing prefix.
        result[base] = (E::ONE - prev.active) * cur.active;
        // Entries continuing an address agree on it.
        let continues = cur.active * cur.same_addr;
        result[base + 1] = continues * (cur.addr - prev.addr);
        // A read of a continuing address returns the previous entry's value.
        let read_continues = continues * (E::ONE - cur.is_write);
        result[base + 2] = read_continues * (cur.lo - prev.lo);
        result[base + 3] = read_continues * (cur.hi - prev.hi);
    }
}

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

use ananse_trace::layout::BUS_SLOTS;
use p3_air::AirBuilder;
use p3_field::{Dup, PrimeCharacteristicRing};
use p3_goldilocks::Goldilocks as Felt;

use crate::bus::SortedEntry;

/// Evaluates the read-consistency residuals on the `local`/`next` row pair.
pub(crate) fn evaluate<AB: AirBuilder<F = Felt>>(
    builder: &mut AB,
    local: &[AB::Var],
    next: &[AB::Var],
) {
    let mut when = builder.when_transition();
    for pair in 0..BUS_SLOTS {
        let prev = SortedEntry::read(local, pair);
        let cur = if pair + 1 < BUS_SLOTS {
            SortedEntry::read(local, pair + 1)
        } else {
            SortedEntry::read(next, 0)
        };
        // No real access may follow a padding entry, so the active flags are a
        // non-increasing prefix.
        when.assert_zero((AB::Expr::ONE - prev.active) * cur.active);
        // Entries continuing an address agree on it.
        let continues: AB::Expr = cur.active * cur.same_addr;
        when.assert_zero(continues.dup() * (cur.addr - prev.addr));
        // A read of a continuing address returns the previous entry's value.
        let read_continues: AB::Expr = continues * (AB::Expr::ONE - cur.is_write);
        when.assert_zero(read_continues.dup() * (cur.lo - prev.lo));
        when.assert_zero(read_continues * (cur.hi - prev.hi));
    }
}

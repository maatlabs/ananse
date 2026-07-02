//! Field-expression views over a constraint row's value bus and sorted access log.

use ananse_trace::layout::{bus_slot, slot, sorted, sorted_slot};
use maat_field::FieldElement;

/// One value-bus slot's columns, read out of a constraint row.
#[derive(Clone, Copy)]
pub(crate) struct BusSlot<E> {
    /// Access address in the unified space.
    pub addr: E,
    /// Low limb of the accessed value.
    pub lo: E,
    /// High limb of the accessed value.
    pub hi: E,
    /// One on a write access, zero on a read.
    pub is_write: E,
    /// One when the slot carries a real access.
    pub active: E,
}

impl<E: FieldElement> BusSlot<E> {
    /// Reads value-bus slot `index` from `row`.
    pub(crate) fn read(row: &[E], index: usize) -> Self {
        let base = bus_slot(index);
        Self {
            addr: row[base + slot::ADDR],
            lo: row[base + slot::LO],
            hi: row[base + slot::HI],
            is_write: row[base + slot::IS_WRITE],
            active: row[base + slot::ACTIVE],
        }
    }
}

/// One sorted-log entry's columns, read out of a constraint row.
#[derive(Clone, Copy)]
pub(crate) struct SortedEntry<E> {
    /// Access address; entries are laid in non-decreasing address order.
    pub addr: E,
    /// Low limb of the accessed value.
    pub lo: E,
    /// High limb of the accessed value.
    pub hi: E,
    /// One on a write access, zero on a read.
    pub is_write: E,
    /// One when the entry is real, zero when it pads the log's tail.
    pub active: E,
    /// One when this entry continues the previous entry's address.
    pub same_addr: E,
}

impl<E: FieldElement> SortedEntry<E> {
    /// Reads sorted-log entry `index` from `row`.
    pub(crate) fn read(row: &[E], index: usize) -> Self {
        let base = sorted_slot(index);
        Self {
            addr: row[base + sorted::ADDR],
            lo: row[base + sorted::LO],
            hi: row[base + sorted::HI],
            is_write: row[base + sorted::IS_WRITE],
            active: row[base + sorted::ACTIVE],
            same_addr: row[base + sorted::SAME_ADDR],
        }
    }
}

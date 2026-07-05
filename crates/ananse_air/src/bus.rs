//! Column views over a constraint row's value bus and sorted access log.

use ananse_trace::layout::{bus_slot, slot, sorted, sorted_slot};

/// One value-bus slot's columns, read out of a constraint row.
#[derive(Clone, Copy)]
pub(crate) struct BusSlot<V> {
    /// Access address in the unified space.
    pub addr: V,
    /// Low limb of the accessed value.
    pub lo: V,
    /// High limb of the accessed value.
    pub hi: V,
    /// One on a write access, zero on a read.
    pub is_write: V,
    /// One when the slot carries a real access.
    pub active: V,
}

impl<V: Copy> BusSlot<V> {
    /// Reads value-bus slot `index` from `row`.
    pub(crate) fn read(row: &[V], index: usize) -> Self {
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

/// One address-sorted log entry's columns, read out of a constraint row.
#[derive(Clone, Copy)]
pub(crate) struct SortedEntry<V> {
    /// Access address; entries are laid in non-decreasing address order.
    pub addr: V,
    /// Timestamp the access carried on the value bus, `clk * BUS_SLOTS + slot`.
    pub ts: V,
    /// Low limb of the accessed value.
    pub lo: V,
    /// High limb of the accessed value.
    pub hi: V,
    /// One on a write access, zero on a read.
    pub is_write: V,
    /// One when the entry is real, zero when it pads the log's tail.
    pub active: V,
    /// One when this entry continues the previous entry's address.
    pub same_addr: V,
}

impl<V: Copy> SortedEntry<V> {
    /// Reads sorted-log entry `index` from `row`.
    pub(crate) fn read(row: &[V], index: usize) -> Self {
        let base = sorted_slot(index);
        Self {
            addr: row[base + sorted::ADDR],
            ts: row[base + sorted::TS],
            lo: row[base + sorted::LO],
            hi: row[base + sorted::HI],
            is_write: row[base + sorted::IS_WRITE],
            active: row[base + sorted::ACTIVE],
            same_addr: row[base + sorted::SAME_ADDR],
        }
    }
}

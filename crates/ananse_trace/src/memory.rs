//! The linear-memory access log: the one permutation-checked structure in the
//! trace.
//!
//! Operand-stack, locals, and globals accesses are static register-column
//! reads and writes and need no permutation argument. Linear memory is the only
//! genuinely dynamic addressing, so its accesses are collected into an
//! address-sorted log the prover permutes against the execution-order columns.

use ananse_executor::StepRecord;

use crate::{Result, TraceError};

/// A single linear-memory access in the address-sorted log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryAccess {
    /// Effective byte address of the access.
    pub address: u64,
    /// Step (row index) at which the access occurred.
    pub step: usize,
    /// Little-endian value read or written, zero-extended to 64 bits.
    pub value: u64,
    /// Whether the access is a store (`true`) or a load (`false`).
    pub is_write: bool,
}

/// Collects every linear-memory access from the record stream into a log sorted
/// by `(address, step)`---the order the permutation argument commits to.
pub(crate) fn access_log(records: &[StepRecord]) -> Vec<MemoryAccess> {
    let mut log = records
        .iter()
        .enumerate()
        .flat_map(|(step, record)| {
            record.memory.iter().map(move |access| MemoryAccess {
                address: access.address,
                step,
                value: access.value,
                is_write: access.store,
            })
        })
        .collect::<Vec<_>>();
    log.sort_by_key(|access| (access.address, access.step));
    log
}

/// Verifies read-consistency over the address-sorted log: within each address's
/// run, every load returns the value established by the most recent store (or the
/// run's first entry). A correct executor always satisfies this.
pub(crate) fn validate(log: &[MemoryAccess]) -> Result<()> {
    let mut iter = log.iter().peekable();
    while let Some(&first) = iter.next() {
        let address = first.address;
        let mut current = first.value;
        while let Some(&&next) = iter.peek().filter(|access| access.address == address) {
            if !next.is_write && next.value != current {
                return Err(TraceError::MemoryInconsistent {
                    address,
                    step: next.step,
                });
            }
            current = next.value;
            iter.next();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(address: u64, step: usize, value: u64, is_write: bool) -> MemoryAccess {
        MemoryAccess {
            address,
            step,
            value,
            is_write,
        }
    }

    #[test]
    fn load_agrees_with_the_preceding_store() {
        let log = vec![access(8, 0, 123, true), access(8, 1, 123, false)];
        assert_eq!(validate(&log), Ok(()));
    }

    #[test]
    fn load_disagreeing_with_the_last_store_is_rejected() {
        let log = vec![access(8, 0, 123, true), access(8, 1, 999, false)];
        assert_eq!(
            validate(&log),
            Err(TraceError::MemoryInconsistent {
                address: 8,
                step: 1,
            })
        );
    }

    #[test]
    fn a_first_load_establishes_the_address_value() {
        // An address only ever loaded takes its value from the first access;
        // later loads of that value are consistent.
        let log = vec![access(4, 0, 7, false), access(4, 3, 7, false)];
        assert_eq!(validate(&log), Ok(()));
    }

    #[test]
    fn addresses_are_validated_independently() {
        let log = vec![
            access(4, 0, 1, true),
            access(4, 2, 1, false),
            access(8, 1, 2, true),
            access(8, 3, 2, false),
        ];
        assert_eq!(validate(&log), Ok(()));
    }
}

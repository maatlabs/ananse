//! Main-segment transition constraints over the unified access-log trace.

use ananse_trace::layout::{
    BUS_SLOTS, COL_CLK, SELECTOR_BASE, bus_slot, slot, sorted, sorted_slot,
};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
use maat_field::{Felt, FieldElement};
use winter_air::TransitionConstraintDegree;

use crate::{address, consistency, numeric};

/// Geometry-independent constraints: one booleanity per opcode selector, the opcode
/// one-hot sum, the padding absorber, and the clock increment.
const FIXED: usize = NUM_SELECTORS + 3;
/// Value-bus flag booleanity constraints: `active` and `is_write` per slot.
const BUS_FLAGS: usize = 2 * BUS_SLOTS;
/// Sorted-log flag booleanity constraints: `active`, `is_write`, and `same_addr`
/// per entry.
const SORTED_FLAGS: usize = 3 * BUS_SLOTS;

/// Total number of main transition constraints.
pub(crate) const NUM_CONSTRAINTS: usize = FIXED
    + BUS_FLAGS
    + SORTED_FLAGS
    + consistency::NUM_CONSTRAINTS
    + address::NUM_CONSTRAINTS
    + numeric::NUM_CONSTRAINTS;

/// Constraint index of the opcode one-hot sum, after the per-selector booleanity
/// constraints.
const OPCODE_ONE_HOT_SUM: usize = NUM_SELECTORS;
/// Constraint index of the padding-absorbing constraint.
const PADDING_ABSORBS: usize = NUM_SELECTORS + 1;
/// Constraint index of the clock-increment constraint.
const CLOCK_STEPS: usize = NUM_SELECTORS + 2;
/// First constraint index of the value-bus flag booleanity block.
const BUS_FLAG_BASE: usize = FIXED;
/// First constraint index of the sorted-log flag booleanity block.
const SORTED_FLAG_BASE: usize = FIXED + BUS_FLAGS;
/// First constraint index of the read-consistency block.
const CONSISTENCY_BASE: usize = FIXED + BUS_FLAGS + SORTED_FLAGS;
/// First constraint index of the address-binding block.
const ADDRESS_BASE: usize = CONSISTENCY_BASE + consistency::NUM_CONSTRAINTS;
/// First constraint index of the numeric value-semantics block.
const NUMERIC_BASE: usize = ADDRESS_BASE + address::NUM_CONSTRAINTS;

/// Algebraic degree of each transition constraint, in the order [`evaluate`] writes
/// them.
pub(crate) fn degrees() -> Vec<TransitionConstraintDegree> {
    (0..NUM_SELECTORS)
        .map(|_| TransitionConstraintDegree::new(2))
        .chain([
            TransitionConstraintDegree::new(1),
            TransitionConstraintDegree::new(2),
            TransitionConstraintDegree::new(1),
        ])
        .chain((0..BUS_FLAGS + SORTED_FLAGS).map(|_| TransitionConstraintDegree::new(2)))
        .chain(consistency::degrees())
        .chain(address::degrees())
        .chain(numeric::degrees())
        .collect()
}

/// Evaluates the main transition constraints on the `current`/`next` row pair,
/// writing one residual per constraint into `result`.
pub(crate) fn evaluate<E: FieldElement<BaseField = Felt>>(
    current: &[E],
    next: &[E],
    stack_base: u64,
    result: &mut [E],
) {
    let selectors = &current[SELECTOR_BASE..SELECTOR_BASE + NUM_SELECTORS];
    for (residual, &selector) in result.iter_mut().zip(selectors) {
        *residual = selector * (selector - E::ONE);
    }
    result[OPCODE_ONE_HOT_SUM] = selectors.iter().copied().fold(E::ZERO, |acc, s| acc + s) - E::ONE;

    let pad = current[SELECTOR_BASE + SEL_PADDING];
    let pad_next = next[SELECTOR_BASE + SEL_PADDING];
    result[PADDING_ABSORBS] = pad * (E::ONE - pad_next);

    result[CLOCK_STEPS] = next[COL_CLK] - current[COL_CLK] - E::ONE;

    bus_flags(current, &mut result[BUS_FLAG_BASE..]);
    sorted_flags(current, &mut result[SORTED_FLAG_BASE..]);
    consistency::evaluate(current, next, &mut result[CONSISTENCY_BASE..]);
    address::evaluate(current, stack_base, &mut result[ADDRESS_BASE..]);
    numeric::evaluate(current, &mut result[NUMERIC_BASE..]);
}

/// Booleanity of every value-bus slot's `active` and `is_write` flags.
fn bus_flags<E: FieldElement<BaseField = Felt>>(current: &[E], result: &mut [E]) {
    for index in 0..BUS_SLOTS {
        let base = bus_slot(index);
        let boolean = |column: usize| {
            let flag = current[base + column];
            flag * (flag - E::ONE)
        };
        result[index * 2] = boolean(slot::ACTIVE);
        result[index * 2 + 1] = boolean(slot::IS_WRITE);
    }
}

/// Booleanity of every sorted-log entry's `active`, `is_write`, and `same_addr`
/// flags.
fn sorted_flags<E: FieldElement<BaseField = Felt>>(current: &[E], result: &mut [E]) {
    for index in 0..BUS_SLOTS {
        let base = sorted_slot(index);
        let boolean = |column: usize| {
            let flag = current[base + column];
            flag * (flag - E::ONE)
        };
        result[index * 3] = boolean(sorted::ACTIVE);
        result[index * 3 + 1] = boolean(sorted::IS_WRITE);
        result[index * 3 + 2] = boolean(sorted::SAME_ADDR);
    }
}

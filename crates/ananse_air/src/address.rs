//! Binding value-bus slot addresses to the static schedule.

use ananse_executor::OpCode;
use ananse_trace::layout::{COL_FRAME_BASE, COL_HEIGHT, REGISTER_REGION, SELECTOR_BASE};
use ananse_trace::selector::opcode_index;
use maat_field::{Felt, FieldElement};
use winter_air::TransitionConstraintDegree;

use crate::bus::BusSlot;

/// Number of address-binding constraints: for the pop-two-push-one shape, three slot
/// addresses, three `active` flags, one idle-slot flag, and three `is_write` flags.
pub(crate) const NUM_CONSTRAINTS: usize = 10;

/// Algebraic degree of each address-binding constraint: the group selector (degree
/// one) times a linear expression in the height, frame base, and a slot column.
pub(crate) fn degrees() -> Vec<TransitionConstraintDegree> {
    (0..NUM_CONSTRAINTS)
        .map(|_| TransitionConstraintDegree::new(2))
        .collect()
}

/// Evaluates the address-binding residuals on `current`, given the frame's public `stack_base`.
pub(crate) fn evaluate<E: FieldElement<BaseField = Felt>>(
    current: &[E],
    stack_base: u64,
    result: &mut [E],
) {
    let selector = |op: OpCode| current[SELECTOR_BASE + opcode_index(op)];
    // The pop-two-push-one arithmetic shape: two operands read from the stack top and
    // the slot beneath it, the result written back to the deeper slot.
    let group = selector(OpCode::I32Add)
        + selector(OpCode::I32Sub)
        + selector(OpCode::I64Add)
        + selector(OpCode::I64Sub);

    let region = E::from(Felt::new(REGISTER_REGION));
    let base = region + current[COL_FRAME_BASE] + E::from(Felt::new(stack_base));
    let height = current[COL_HEIGHT];
    let top = base + height - E::ONE;
    let deep = base + height - E::from(Felt::new(2));

    let c2 = BusSlot::read(current, 0);
    let c1 = BusSlot::read(current, 1);
    let r = BusSlot::read(current, 2);
    let idle = BusSlot::read(current, 3);

    // Addresses: top operand, deeper operand, result over the deeper slot.
    result[0] = group * (c2.addr - top);
    result[1] = group * (c1.addr - deep);
    result[2] = group * (r.addr - deep);
    // The three accessed slots are active and the fourth is idle.
    result[3] = group * (c2.active - E::ONE);
    result[4] = group * (c1.active - E::ONE);
    result[5] = group * (r.active - E::ONE);
    result[6] = group * idle.active;
    // The two operands are reads and the result is a write.
    result[7] = group * c2.is_write;
    result[8] = group * c1.is_write;
    result[9] = group * (r.is_write - E::ONE);
}

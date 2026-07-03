//! Binding value-bus slot addresses to the static schedule.

use ananse_executor::OpCode;
use ananse_trace::layout::{COL_FRAME_BASE, COL_HEIGHT, REGISTER_REGION, SELECTOR_BASE};
use ananse_trace::selector::opcode_index;
use p3_air::AirBuilder;
use p3_field::Dup;
use p3_goldilocks::Goldilocks as Felt;

use crate::bus::BusSlot;

pub(crate) fn evaluate<AB: AirBuilder<F = Felt>>(
    builder: &mut AB,
    local: &[AB::Var],
    stack_base: u64,
) {
    let selector = |op: OpCode| local[SELECTOR_BASE + opcode_index(op)];
    // The pop-two-push-one arithmetic shape: two operands read from the stack top and
    // the slot beneath it, the result written back to the deeper slot.
    let group: AB::Expr = selector(OpCode::I32Add)
        + selector(OpCode::I32Sub)
        + selector(OpCode::I64Add)
        + selector(OpCode::I64Sub);

    let base: AB::Expr = local[COL_FRAME_BASE] + Felt::new(REGISTER_REGION) + Felt::new(stack_base);
    let height = local[COL_HEIGHT];
    let top: AB::Expr = base.dup() + height - Felt::new(1);
    let deep: AB::Expr = base + height - Felt::new(2);

    let c2 = BusSlot::read(local, 0);
    let c1 = BusSlot::read(local, 1);
    let r = BusSlot::read(local, 2);
    let idle = BusSlot::read(local, 3);

    // Addresses: top operand, deeper operand, result over the deeper slot.
    builder.assert_zero(group.dup() * (c2.addr - top));
    builder.assert_zero(group.dup() * (c1.addr - deep.dup()));
    builder.assert_zero(group.dup() * (r.addr - deep));
    // The three accessed slots are active and the fourth is idle.
    builder.assert_zero(group.dup() * (c2.active - Felt::new(1)));
    builder.assert_zero(group.dup() * (c1.active - Felt::new(1)));
    builder.assert_zero(group.dup() * (r.active - Felt::new(1)));
    builder.assert_zero(group.dup() * idle.active);
    // The two operands are reads and the result is a write.
    builder.assert_zero(group.dup() * c2.is_write);
    builder.assert_zero(group.dup() * c1.is_write);
    builder.assert_zero(group * (r.is_write - Felt::new(1)));
}

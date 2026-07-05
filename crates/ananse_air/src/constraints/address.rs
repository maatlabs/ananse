//! Binding value-bus slot addresses to the static schedule.

use ananse_executor::OpCode;
use ananse_trace::layout::{COL_FRAME_BASE, COL_HEIGHT, COL_IMM, REGISTER_REGION, SELECTOR_BASE};
use ananse_trace::selector::opcode_index;
use p3_air::AirBuilder;
use p3_field::{Dup, PrimeCharacteristicRing};

use super::bitwise::BINARY as BITWISE;
use super::comparison::BINARY as COMPARISON;
use crate::Felt;
use crate::bus::BusSlot;

pub(crate) fn evaluate<AB: AirBuilder<F = Felt>>(
    builder: &mut AB,
    local: &[AB::Var],
    stack_base: u64,
) {
    let selector = |op: OpCode| local[SELECTOR_BASE + opcode_index(op)];
    let bank = || local[COL_FRAME_BASE] + Felt::new(REGISTER_REGION) + local[COL_IMM];
    let new_top = || {
        local[COL_FRAME_BASE]
            + Felt::new(REGISTER_REGION)
            + Felt::new(stack_base)
            + local[COL_HEIGHT]
    };
    let pop_top = || {
        local[COL_FRAME_BASE]
            + Felt::new(REGISTER_REGION)
            + Felt::new(stack_base)
            + local[COL_HEIGHT]
            - Felt::new(1)
    };

    let group: AB::Expr = selector(OpCode::I32Add)
        + selector(OpCode::I32Sub)
        + selector(OpCode::I64Add)
        + selector(OpCode::I64Sub)
        + COMPARISON
            .iter()
            .chain(BITWISE.iter())
            .fold(AB::Expr::ZERO, |acc, &op| acc + selector(op));

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

    let s0 = BusSlot::read(local, 0);
    let s1 = BusSlot::read(local, 1);
    let s2 = BusSlot::read(local, 2);
    let s3 = BusSlot::read(local, 3);

    // `local.get` / `global.get`: read the named bank slot, push it as the new stack
    // top.
    let read_push: AB::Expr = selector(OpCode::LocalGet) + selector(OpCode::GlobalGet);
    builder.assert_zero(read_push.dup() * (s0.addr - bank()));
    builder.assert_zero(read_push.dup() * (s0.active - Felt::new(1)));
    builder.assert_zero(read_push.dup() * s0.is_write);
    builder.assert_zero(read_push.dup() * (s1.addr - new_top()));
    builder.assert_zero(read_push.dup() * (s1.active - Felt::new(1)));
    builder.assert_zero(read_push.dup() * (s1.is_write - Felt::new(1)));
    builder.assert_zero(read_push.dup() * s2.active);
    builder.assert_zero(read_push * s3.active);

    // `i32.const` / `i64.const`: push the immediate as the new stack top.
    let push_const: AB::Expr = selector(OpCode::I32Const) + selector(OpCode::I64Const);
    builder.assert_zero(push_const.dup() * (s0.addr - new_top()));
    builder.assert_zero(push_const.dup() * (s0.active - Felt::new(1)));
    builder.assert_zero(push_const.dup() * (s0.is_write - Felt::new(1)));
    builder.assert_zero(push_const.dup() * s1.active);
    builder.assert_zero(push_const.dup() * s2.active);
    builder.assert_zero(push_const * s3.active);

    // `local.set` / `global.set` / `local.tee`: read the stack top, write the named
    // bank slot. (`tee` leaves the top in place, which changes no bus access.)
    let pop_write: AB::Expr =
        selector(OpCode::LocalSet) + selector(OpCode::GlobalSet) + selector(OpCode::LocalTee);
    builder.assert_zero(pop_write.dup() * (s0.addr - pop_top()));
    builder.assert_zero(pop_write.dup() * (s0.active - Felt::new(1)));
    builder.assert_zero(pop_write.dup() * s0.is_write);
    builder.assert_zero(pop_write.dup() * (s1.addr - bank()));
    builder.assert_zero(pop_write.dup() * (s1.active - Felt::new(1)));
    builder.assert_zero(pop_write.dup() * (s1.is_write - Felt::new(1)));
    builder.assert_zero(pop_write.dup() * s2.active);
    builder.assert_zero(pop_write * s3.active);

    // The pop-one-read shape: read the stack top and touch nothing else. `drop`
    // discards it; `if` / `br_if` read the popped condition that steers control.
    let pop_read: AB::Expr = selector(OpCode::Drop) + selector(OpCode::If) + selector(OpCode::BrIf);
    builder.assert_zero(pop_read.dup() * (s0.addr - pop_top()));
    builder.assert_zero(pop_read.dup() * (s0.active - Felt::new(1)));
    builder.assert_zero(pop_read.dup() * s0.is_write);
    builder.assert_zero(pop_read.dup() * s1.active);
    builder.assert_zero(pop_read.dup() * s2.active);
    builder.assert_zero(pop_read * s3.active);

    // The pop-one-push-one shape: read the stack top and write the result back over
    // it. `eqz`, `popcnt`, and the three conversions all touch the top slot alone.
    let unary: AB::Expr = selector(OpCode::I32WrapI64)
        + selector(OpCode::I64ExtendI32U)
        + selector(OpCode::I64ExtendI32S)
        + selector(OpCode::I32Eqz)
        + selector(OpCode::I64Eqz)
        + selector(OpCode::I32Popcnt)
        + selector(OpCode::I64Popcnt);
    builder.assert_zero(unary.dup() * (s0.addr - pop_top()));
    builder.assert_zero(unary.dup() * (s0.active - Felt::new(1)));
    builder.assert_zero(unary.dup() * s0.is_write);
    builder.assert_zero(unary.dup() * (s1.addr - pop_top()));
    builder.assert_zero(unary.dup() * (s1.active - Felt::new(1)));
    builder.assert_zero(unary.dup() * (s1.is_write - Felt::new(1)));
    builder.assert_zero(unary.dup() * s2.active);
    builder.assert_zero(unary * s3.active);

    // `select` reads three operands and writes one: slot 0 the condition on top,
    // slot 1 `val2` beneath it, slot 2 `val1` below that, and slot 3 the result
    // written back over `val1`'s deepest slot.
    let select: AB::Expr = selector(OpCode::Select).into();
    builder.assert_zero(select.dup() * (s0.addr - pop_top()));
    builder.assert_zero(select.dup() * (s1.addr - (pop_top() - Felt::new(1))));
    builder.assert_zero(select.dup() * (s2.addr - (pop_top() - Felt::new(2))));
    builder.assert_zero(select.dup() * (s3.addr - (pop_top() - Felt::new(2))));
    builder.assert_zero(select.dup() * (s0.active - Felt::new(1)));
    builder.assert_zero(select.dup() * (s1.active - Felt::new(1)));
    builder.assert_zero(select.dup() * (s2.active - Felt::new(1)));
    builder.assert_zero(select.dup() * (s3.active - Felt::new(1)));
    builder.assert_zero(select.dup() * s0.is_write);
    builder.assert_zero(select.dup() * s1.is_write);
    builder.assert_zero(select.dup() * s2.is_write);
    builder.assert_zero(select * (s3.is_write - Felt::new(1)));
}

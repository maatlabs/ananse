//! Data-movement value families: the copy and truncation relations over the value bus.
//!
//! The copy operators---`local.get`/`set`/`tee` and `global.get`/`set`---move a value
//! between a register bank slot and the operand stack without transforming it, so the
//! written slot equals the read slot limb for limb. The width-narrowing conversions
//! `i32.wrap_i64` and `i64.extend_i32_u` keep the low limb and clear the high one:
//! `wrap` discards the high 32 bits, and an unsigned extension of an already-32-bit
//! operand contributes no high bits. Every family reads its source on bus slot 0 and
//! writes its result on slot 1; the addresses of those slots are bound to the schedule
//! by [`crate::constraints::address`].

use ananse_executor::OpCode;
use ananse_trace::layout::SELECTOR_BASE;
use ananse_trace::selector::opcode_index;
use p3_air::AirBuilder;
use p3_field::Dup;
use p3_goldilocks::Goldilocks as Felt;

use crate::bus::BusSlot;

pub(crate) fn evaluate<AB: AirBuilder<F = Felt>>(builder: &mut AB, local: &[AB::Var]) {
    let selector = |op: OpCode| local[SELECTOR_BASE + opcode_index(op)];
    let source = BusSlot::read(local, 0);
    let result = BusSlot::read(local, 1);

    // The copy operators write the value they read, unchanged.
    let copy: AB::Expr = selector(OpCode::LocalGet)
        + selector(OpCode::GlobalGet)
        + selector(OpCode::LocalSet)
        + selector(OpCode::GlobalSet)
        + selector(OpCode::LocalTee);
    builder.assert_zero(copy.dup() * (result.lo - source.lo));
    builder.assert_zero(copy * (result.hi - source.hi));

    // `wrap` and unsigned `extend` keep the low limb and clear the high one.
    let truncate: AB::Expr = selector(OpCode::I32WrapI64) + selector(OpCode::I64ExtendI32U);
    builder.assert_zero(truncate.dup() * (result.lo - source.lo));
    builder.assert_zero(truncate * result.hi);
}

//! The numeric constraint family: integer arithmetic over the two-limb value-bus
//! encoding.

use ananse_executor::OpCode;
use ananse_trace::layout::SELECTOR_BASE;
use ananse_trace::selector::opcode_index;
use p3_air::AirBuilder;
use p3_field::{Dup, Field};
use p3_goldilocks::Goldilocks as Felt;

use crate::bus::BusSlot;

pub(crate) fn evaluate<AB: AirBuilder<F = Felt>>(builder: &mut AB, local: &[AB::Var]) {
    let two32 = Felt::new(1u64 << 32);
    let inv32 = two32.inverse();
    let sel = |op: OpCode| local[SELECTOR_BASE + opcode_index(op)];
    let sel_add32 = sel(OpCode::I32Add);
    let sel_sub32 = sel(OpCode::I32Sub);
    let sel_add64 = sel(OpCode::I64Add);
    let sel_sub64 = sel(OpCode::I64Sub);

    let c2 = BusSlot::read(local, 0);
    let c1 = BusSlot::read(local, 1);
    let r = BusSlot::read(local, 2);

    let add_balance: AB::Expr = c1.lo + c2.lo - r.lo;
    let sub_balance: AB::Expr = c1.lo - c2.lo - r.lo;

    builder.assert_zero((sel_add32 + sel_add64) * add_balance.dup() * (add_balance.dup() - two32));
    builder.assert_zero((sel_sub32 + sel_sub64) * sub_balance.dup() * (sub_balance.dup() + two32));
    builder.assert_zero((sel_add32 + sel_sub32) * r.hi);

    let carry = add_balance * inv32;
    let add_hi = c1.hi + c2.hi + carry - r.hi;
    builder.assert_zero(sel_add64 * add_hi.dup() * (add_hi - two32));

    let borrow = -sub_balance * inv32;
    let sub_hi = c1.hi - c2.hi - borrow - r.hi;
    builder.assert_zero(sel_sub64 * sub_hi.dup() * (sub_hi + two32));
}

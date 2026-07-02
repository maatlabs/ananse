//! The numeric constraint family: integer arithmetic over the two-limb value-bus
//! encoding.

use ananse_executor::OpCode;
use ananse_trace::layout::SELECTOR_BASE;
use ananse_trace::selector::opcode_index;
use maat_field::{Felt, FieldElement};
use winter_air::TransitionConstraintDegree;

use crate::bus::BusSlot;

/// Number of numeric transition constraints.
pub(crate) const NUM_CONSTRAINTS: usize = 5;

/// Low-limb addition balance, shared by `i32.add` and `i64.add`.
const ADD_LO: usize = 0;
/// Low-limb subtraction balance, shared by `i32.sub` and `i64.sub`.
const SUB_LO: usize = 1;
/// The `i32` result high limb is zero (shared by `i32.add` and `i32.sub`).
const I32_RESULT_HI: usize = 2;
/// High-limb addition balance for `i64.add`, carrying the low limb's overflow.
const I64_ADD_HI: usize = 3;
/// High-limb subtraction balance for `i64.sub`, carrying the low limb's borrow.
const I64_SUB_HI: usize = 4;

/// Algebraic degree of each numeric constraint, in the order [`evaluate`] writes
/// them. A limb balance multiplies the opcode selector (degree one) by the quadratic
/// balance (degree two), so it is degree three; the `i32` result-high zeroing is
/// degree two.
pub(crate) fn degrees() -> Vec<TransitionConstraintDegree> {
    [3, 3, 2, 3, 3]
        .into_iter()
        .map(TransitionConstraintDegree::new)
        .collect()
}

/// Evaluates the numeric family on `current`, writing one residual per constraint
/// into `result`. Each residual is gated by its opcode selector, so it vanishes off
/// the opcode's own rows.
pub(crate) fn evaluate<E: FieldElement<BaseField = Felt>>(current: &[E], result: &mut [E]) {
    let two32 = E::from(Felt::new(1u64 << 32));
    let inv32 = two32.inv();
    let selector = |op: OpCode| current[SELECTOR_BASE + opcode_index(op)];
    let (sel_add32, sel_sub32) = (selector(OpCode::I32Add), selector(OpCode::I32Sub));
    let (sel_add64, sel_sub64) = (selector(OpCode::I64Add), selector(OpCode::I64Sub));

    let c2 = BusSlot::read(current, 0);
    let c1 = BusSlot::read(current, 1);
    let r = BusSlot::read(current, 2);

    let add_balance = c1.lo + c2.lo - r.lo;
    let sub_balance = c1.lo - c2.lo - r.lo;
    result[ADD_LO] = (sel_add32 + sel_add64) * add_balance * (add_balance - two32);
    result[SUB_LO] = (sel_sub32 + sel_sub64) * sub_balance * (sub_balance + two32);
    result[I32_RESULT_HI] = (sel_add32 + sel_sub32) * r.hi;

    let carry = add_balance * inv32;
    let add_hi = c1.hi + c2.hi + carry - r.hi;
    result[I64_ADD_HI] = sel_add64 * add_hi * (add_hi - two32);

    let borrow = -sub_balance * inv32;
    let sub_hi = c1.hi - c2.hi - borrow - r.hi;
    result[I64_SUB_HI] = sel_sub64 * sub_hi * (sub_hi + two32);
}

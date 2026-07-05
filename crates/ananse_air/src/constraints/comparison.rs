//! Integer comparison, `eqz`, and signed sign-extension value relations.

use ananse_executor::OpCode;
use ananse_trace::layout::{
    LIMB_BYTES, RC_CMP_DHI, RC_CMP_DLO, RC_SIGN_A, RC_SIGN_B, SELECTOR_BASE, wit, witness,
};
use ananse_trace::selector::opcode_index;
use p3_air::AirBuilder;
use p3_field::{Dup, PrimeCharacteristicRing};

use crate::Felt;
use crate::bus::BusSlot;

/// The twenty binary comparison operators.
pub(crate) const BINARY: [OpCode; 20] = [
    OpCode::I32Eq,
    OpCode::I32Ne,
    OpCode::I32LtS,
    OpCode::I32LtU,
    OpCode::I32GtS,
    OpCode::I32GtU,
    OpCode::I32LeS,
    OpCode::I32LeU,
    OpCode::I32GeS,
    OpCode::I32GeU,
    OpCode::I64Eq,
    OpCode::I64Ne,
    OpCode::I64LtS,
    OpCode::I64LtU,
    OpCode::I64GtS,
    OpCode::I64GtU,
    OpCode::I64LeS,
    OpCode::I64LeU,
    OpCode::I64GeS,
    OpCode::I64GeU,
];
const EQ: [OpCode; 2] = [OpCode::I32Eq, OpCode::I64Eq];
const NE: [OpCode; 2] = [OpCode::I32Ne, OpCode::I64Ne];
const LT_U: [OpCode; 2] = [OpCode::I32LtU, OpCode::I64LtU];
const GT_U: [OpCode; 2] = [OpCode::I32GtU, OpCode::I64GtU];
const LE_U: [OpCode; 2] = [OpCode::I32LeU, OpCode::I64LeU];
const GE_U: [OpCode; 2] = [OpCode::I32GeU, OpCode::I64GeU];
const LT_S: [OpCode; 2] = [OpCode::I32LtS, OpCode::I64LtS];
const GT_S: [OpCode; 2] = [OpCode::I32GtS, OpCode::I64GtS];
const LE_S: [OpCode; 2] = [OpCode::I32LeS, OpCode::I64LeS];
const GE_S: [OpCode; 2] = [OpCode::I32GeS, OpCode::I64GeS];
const EQZ: [OpCode; 2] = [OpCode::I32Eqz, OpCode::I64Eqz];
/// The operators that pop an operand and branch on whether it is zero: `select`
/// muxes on it, `if` and `br_if` steer control by it. They borrow this file's
/// is-zero gadget to pin `condition == 0` into the shared equality bit.
pub(crate) const COND: [OpCode; 3] = [OpCode::Select, OpCode::If, OpCode::BrIf];
const SIGNED32: [OpCode; 4] = [
    OpCode::I32LtS,
    OpCode::I32GtS,
    OpCode::I32LeS,
    OpCode::I32GeS,
];
const SIGNED64: [OpCode; 4] = [
    OpCode::I64LtS,
    OpCode::I64GtS,
    OpCode::I64LeS,
    OpCode::I64GeS,
];

pub(crate) fn evaluate<AB: AirBuilder<F = Felt>>(builder: &mut AB, local: &[AB::Var]) {
    let two32 = Felt::new(1u64 << 32);
    let two31 = Felt::new(1u64 << 31);
    let mask32 = Felt::new(0xFFFF_FFFF);

    let sel = |op: OpCode| local[SELECTOR_BASE + opcode_index(op)];
    let sum = |ops: &[OpCode]| -> AB::Expr {
        ops.iter().fold(AB::Expr::ZERO, |acc, &op| {
            acc + local[SELECTOR_BASE + opcode_index(op)]
        })
    };
    let bytes = |base: usize, count: usize| -> AB::Expr {
        (0..count).fold(AB::Expr::ZERO, |acc, b| {
            acc + local[base + b] * Felt::new(1u64 << (8 * b))
        })
    };

    // Bus roles: on a binary comparison slot 0 is the top operand `rhs`, slot 1 the
    // deeper `lhs`, slot 2 the result; on `eqz` / `extend_s` slot 0 is the operand
    // and slot 1 the result.
    let s0 = BusSlot::read(local, 0);
    let s1 = BusSlot::read(local, 1);
    let s2 = BusSlot::read(local, 2);

    let borrow_lo = local[wit(witness::BORROW_LO)];
    let borrow_hi = local[wit(witness::BORROW_HI)];
    let sign_a = local[wit(witness::SIGN_A)];
    let sign_b = local[wit(witness::SIGN_B)];
    let eqbit = local[wit(witness::EQUAL)];
    let inv = local[wit(witness::INV)];

    // Borrow subtraction `lhs - rhs`: each limb difference lands in `[0, 2^32)` and
    // the outgoing borrows are Boolean, so `borrow_hi` is exactly `lhs < rhs`.
    builder.assert_zero(
        sum(&BINARY) * (s1.lo - s0.lo + borrow_lo * two32 - bytes(RC_CMP_DLO, LIMB_BYTES)),
    );
    builder.assert_zero(
        sum(&BINARY)
            * (s1.hi - s0.hi - borrow_lo + borrow_hi * two32 - bytes(RC_CMP_DHI, LIMB_BYTES)),
    );
    builder.assert_zero(sum(&BINARY) * borrow_lo * (AB::Expr::ONE - borrow_lo));
    builder.assert_zero(sum(&BINARY) * borrow_hi * (AB::Expr::ONE - borrow_hi));

    // Is-zero on the routed field value: the two-limb difference for a binary
    // comparison, the operand itself for `eqz`, the popped condition (bus slot 0)
    // for `select` / `if` / `br_if`.
    let zero_test = sum(&EQZ) + sum(&COND);
    let g = sum(&BINARY) * ((s1.lo - s0.lo) + (s1.hi - s0.hi) * two32)
        + zero_test.dup() * (s0.lo + s0.hi * two32);
    let cmp = sum(&BINARY) + zero_test;
    builder.assert_zero(cmp.dup() * (g.dup() * inv + eqbit - AB::Expr::ONE));
    builder.assert_zero(cmp * g * eqbit);

    // Sign extraction. Operand A is the left comparison operand or the extension
    // source; operand B is the right comparison operand. Each sign-carrying limb is
    // the low limb for a 32-bit operator and the high limb for a 64-bit one.
    let sign_limb_a =
        sum(&SIGNED32) * s1.lo + sum(&SIGNED64) * s1.hi + sel(OpCode::I64ExtendI32S) * s0.lo;
    let uses_a = sum(&SIGNED32) + sum(&SIGNED64) + sel(OpCode::I64ExtendI32S);
    builder
        .assert_zero(uses_a.dup() * (sign_limb_a - sign_a * two31 - bytes(RC_SIGN_A, LIMB_BYTES)));
    builder.assert_zero(
        uses_a.dup()
            * (local[RC_SIGN_A + LIMB_BYTES] - local[RC_SIGN_A + LIMB_BYTES - 1] * Felt::new(2)),
    );
    builder.assert_zero(uses_a * sign_a * (AB::Expr::ONE - sign_a));

    let sign_limb_b = sum(&SIGNED32) * s0.lo + sum(&SIGNED64) * s0.hi;
    let signed = sum(&SIGNED32) + sum(&SIGNED64);
    builder
        .assert_zero(signed.dup() * (sign_limb_b - sign_b * two31 - bytes(RC_SIGN_B, LIMB_BYTES)));
    builder.assert_zero(
        signed.dup()
            * (local[RC_SIGN_B + LIMB_BYTES] - local[RC_SIGN_B + LIMB_BYTES - 1] * Felt::new(2)),
    );
    builder.assert_zero(signed * sign_b * (AB::Expr::ONE - sign_b));

    // Signed less-than is the unsigned bit exclusive-or'd with the two sign bits.
    let sign_xor = sign_a + sign_b - sign_a * sign_b * Felt::new(2);
    let lt_s = borrow_hi + sign_xor.dup() - borrow_hi * sign_xor * Felt::new(2);

    // Every comparison result is an `i32`, so its high limb is zero, and its low limb
    // is the family's Boolean combination of the borrow and equality bits.
    builder.assert_zero(sum(&BINARY) * s2.hi);
    builder.assert_zero(sum(&EQ) * (s2.lo - eqbit));
    builder.assert_zero(sum(&NE) * (s2.lo - (AB::Expr::ONE - eqbit)));
    builder.assert_zero(sum(&LT_U) * (s2.lo - borrow_hi));
    builder.assert_zero(sum(&GE_U) * (s2.lo - (AB::Expr::ONE - borrow_hi)));
    builder.assert_zero(sum(&LE_U) * (s2.lo - (borrow_hi + eqbit)));
    builder.assert_zero(sum(&GT_U) * (s2.lo - (AB::Expr::ONE - borrow_hi - eqbit)));
    builder.assert_zero(sum(&LT_S) * (s2.lo - lt_s.dup()));
    builder.assert_zero(sum(&GE_S) * (s2.lo - (AB::Expr::ONE - lt_s.dup())));
    builder.assert_zero(sum(&LE_S) * (s2.lo - (lt_s.dup() + eqbit)));
    builder.assert_zero(sum(&GT_S) * (s2.lo - (AB::Expr::ONE - lt_s - eqbit)));

    // `eqz` writes the equality-with-zero bit as an `i32`.
    builder.assert_zero(sum(&EQZ) * s1.hi);
    builder.assert_zero(sum(&EQZ) * (s1.lo - eqbit));

    // `i64.extend_i32_s` keeps the low limb and fills the high limb from the sign.
    builder.assert_zero(sel(OpCode::I64ExtendI32S) * (s1.lo - s0.lo));
    builder.assert_zero(sel(OpCode::I64ExtendI32S) * (s1.hi - sign_a * mask32));
}

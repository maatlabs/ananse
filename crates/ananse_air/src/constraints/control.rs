//! Condition-driven mux, branch direction, and immediate binding.

use std::collections::HashMap;

use ananse_executor::OpCode;
use ananse_trace::layout::{COL_PC, PC_DATA_A, PC_DATA_B, SELECTOR_BASE, wit, witness};
use ananse_trace::selector::opcode_index;
use p3_air::{AirBuilder, ExtensionBuilder, PermutationAirBuilder, WindowAccess};
use p3_field::{Dup, Field, PrimeCharacteristicRing, PrimeField64};
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;

use super::{
    AUX_DATA_CHANNEL, AUX_DATA_MULT, AUX_DATA_TABLE, CHALLENGE_DATA_DENOM, CHALLENGE_DATA_FOLD,
};
use crate::bus::BusSlot;
use crate::{AirError, Felt, QuadExt, Result};

/// The two `const` pushes, whose written value the data-ROM binds to the module
/// immediate.
const CONST: [OpCode; 2] = [OpCode::I32Const, OpCode::I64Const];

/// Program points whose `(pc, a, b)` tuple the data-ROM lookup binds: the `const`
/// pushes carry the immediate's limbs, the conditional branches their two targets.
const LOOKED_UP: [OpCode; 4] = [OpCode::I32Const, OpCode::I64Const, OpCode::If, OpCode::BrIf];

pub(crate) fn evaluate<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB) {
    let main = builder.main();
    let local = main.current_slice();
    let next = main.next_slice();

    value_relations(builder, local, next);

    let fold: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_DATA_FOLD].into();
    let denom: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_DATA_DENOM].into();
    // The table side folds each data entry's `(pc, a, b)` from its three periodic
    // columns (indices 9, 10, 11).
    let table = fold_tuple::<AB>(
        fold.dup(),
        [
            builder.periodic_values()[9].into(),
            builder.periodic_values()[10].into(),
            builder.periodic_values()[11].into(),
        ],
    );
    // The channel fires only on a bound row; on every other row its numerator is zero.
    let gate: AB::Expr = LOOKED_UP.iter().fold(AB::Expr::ZERO, |acc, &op| {
        acc + local[SELECTOR_BASE + opcode_index(op)]
    });
    let tuple = fold_tuple::<AB>(
        fold.dup(),
        [
            local[COL_PC].into(),
            local[PC_DATA_A].into(),
            local[PC_DATA_B].into(),
        ],
    );

    let perm = builder.permutation();
    let s_cur = perm.current_slice()[AUX_DATA_CHANNEL];
    let s_next: AB::ExprEF = perm.next_slice()[AUX_DATA_CHANNEL].into();
    let s_delta: AB::ExprEF = s_next - Into::<AB::ExprEF>::into(s_cur);
    builder
        .when_transition()
        .assert_zero_ext(s_delta * (denom.dup() - tuple) - AB::ExprEF::ONE * gate);

    let m_cur = perm.current_slice()[AUX_DATA_MULT];
    let sm_cur = perm.current_slice()[AUX_DATA_TABLE];
    let sm_next: AB::ExprEF = perm.next_slice()[AUX_DATA_TABLE].into();
    let sm_delta: AB::ExprEF = sm_next - Into::<AB::ExprEF>::into(sm_cur);
    builder
        .when_transition()
        .assert_zero_ext(sm_delta * (denom - table) - Into::<AB::ExprEF>::into(m_cur));

    // Both accumulators open at zero and the balance closes at zero, so every bound
    // row's tuple is a data-ROM member: the datum it carries is the committed one.
    builder.when_first_row().assert_zero_ext(s_cur);
    builder.when_first_row().assert_zero_ext(sm_cur);
    builder
        .when_last_row()
        .assert_zero_ext(Into::<AB::ExprEF>::into(s_cur) - Into::<AB::ExprEF>::into(sm_cur));
}

fn value_relations<AB: AirBuilder<F = Felt>>(
    builder: &mut AB,
    local: &[AB::Var],
    next: &[AB::Var],
) {
    let selector = |op: OpCode| local[SELECTOR_BASE + opcode_index(op)];
    let eqbit = local[wit(witness::EQUAL)];
    let mux = |taken: AB::Var, other: AB::Var| (AB::Expr::ONE - eqbit) * taken + eqbit * other;

    let val2 = BusSlot::read(local, 1);
    let val1 = BusSlot::read(local, 2);
    let result = BusSlot::read(local, 3);
    let sel = selector(OpCode::Select);
    builder.assert_zero(sel * (result.lo - mux(val1.lo, val2.lo)));
    builder.assert_zero(sel * (result.hi - mux(val1.hi, val2.hi)));

    let pushed = BusSlot::read(local, 0);
    let is_const: AB::Expr = CONST
        .iter()
        .fold(AB::Expr::ZERO, |acc, &op| acc + selector(op));
    builder.assert_zero(is_const.dup() * (pushed.lo - local[PC_DATA_A]));
    builder.assert_zero(is_const * (pushed.hi - local[PC_DATA_B]));

    let is_branch: AB::Expr = selector(OpCode::If) + selector(OpCode::BrIf);
    builder.assert_zero(is_branch * (next[COL_PC] - mux(local[PC_DATA_A], local[PC_DATA_B])));
}

/// Folds a data tuple `(pc, a, b)` into `pc + a*fold + b*fold^2`.
fn fold_tuple<AB: ExtensionBuilder<F = Felt>>(
    fold: AB::ExprEF,
    terms: [AB::Expr; 3],
) -> AB::ExprEF {
    terms
        .into_iter()
        .rev()
        .fold(AB::ExprEF::ZERO, |acc, term| acc * fold.dup() + term)
}

/// The data-ROM's three periodic columns `(pc, a, b)`, one entry per bound program
/// point at rows `0..data.len()`, zero-padded to `length`.
pub fn data_periodic(data: &[(u32, u32, u32)], length: usize) -> [Vec<Felt>; 3] {
    let column = |pick: fn((u32, u32, u32)) -> u32| -> Vec<Felt> {
        (0..length)
            .map(|row| Felt::new(data.get(row).map_or(0, |&entry| u64::from(pick(entry)))))
            .collect()
    };
    [
        column(|(pc, _, _)| pc),
        column(|(_, a, _)| a),
        column(|(_, _, b)| b),
    ]
}

pub(crate) fn columns(
    main: &RowMajorMatrix<Felt>,
    data: &[(u32, u32, u32)],
    fold: QuadExt,
    denom: QuadExt,
) -> Result<Vec<Vec<QuadExt>>> {
    let height = main.height();
    let width = main.width();
    let row = |r: usize| &main.values[r * width..(r + 1) * width];
    let summed = height.saturating_sub(1);

    if data.len() > summed {
        return Err(AirError::TraceTooShortForRom {
            trace_len: height,
            rom_len: data.len(),
        });
    }

    let index: HashMap<(u64, u64, u64), usize> = data
        .iter()
        .enumerate()
        .map(|(pos, &(pc, a, b))| ((u64::from(pc), u64::from(a), u64::from(b)), pos))
        .collect();

    let fold_row = |pc: Felt, a: Felt, b: Felt| {
        [pc, a, b]
            .into_iter()
            .rev()
            .fold(QuadExt::ZERO, |acc, term| acc * fold + QuadExt::from(term))
    };
    let reciprocal = |value: QuadExt| value.try_inverse().ok_or(AirError::DegenerateChallenge);
    let looked_up = |r: usize| {
        LOOKED_UP
            .iter()
            .any(|&op| row(r)[SELECTOR_BASE + opcode_index(op)] == Felt::ONE)
    };

    // Tally each bound row's lookup against the data entry keyed by its `(pc, a, b)`.
    let mut counts = vec![0u64; data.len()];
    for r in (0..summed).filter(|&r| looked_up(r)) {
        let key = (
            row(r)[COL_PC].as_canonical_u64(),
            row(r)[PC_DATA_A].as_canonical_u64(),
            row(r)[PC_DATA_B].as_canonical_u64(),
        );
        let position = index.get(&key).ok_or_else(|| {
            AirError::LookupBuild(format!("row {r} looks up a datum absent from the data ROM"))
        })?;
        counts[*position] = counts[*position].saturating_add(1);
    }

    let mut multiplicity = vec![QuadExt::ZERO; height];
    for (position, &count) in counts.iter().enumerate() {
        multiplicity[position] = QuadExt::from(Felt::new(count));
    }

    let table_at = |i: usize| -> (Felt, Felt, Felt) {
        data.get(i)
            .map_or((Felt::ZERO, Felt::ZERO, Felt::ZERO), |&(pc, a, b)| {
                (
                    Felt::new(u64::from(pc)),
                    Felt::new(u64::from(a)),
                    Felt::new(u64::from(b)),
                )
            })
    };

    let mut table_sum = vec![QuadExt::ZERO; height];
    for i in 0..summed {
        let (pc, a, b) = table_at(i);
        table_sum[i + 1] = table_sum[i] + multiplicity[i] * reciprocal(denom - fold_row(pc, a, b))?;
    }

    let mut channel = vec![QuadExt::ZERO; height];
    for r in 0..summed {
        let step = if looked_up(r) {
            reciprocal(denom - fold_row(row(r)[COL_PC], row(r)[PC_DATA_A], row(r)[PC_DATA_B]))?
        } else {
            QuadExt::ZERO
        };
        channel[r + 1] = channel[r] + step;
    }

    Ok(vec![multiplicity, table_sum, channel])
}

//! `i32.popcnt` / `i64.popcnt`: population count through a per-byte lookup.

use std::iter::once;

use ananse_executor::OpCode;
use ananse_trace::layout::{
    LIMB_BYTES, POPCNT_BYTE_BASE, POPCNT_BYTES, POPCNT_PC_BASE, SELECTOR_BASE,
};
use ananse_trace::selector::opcode_index;
use p3_air::{ExtensionBuilder, PermutationAirBuilder, WindowAccess};
use p3_field::{Dup, Field, PrimeCharacteristicRing, PrimeField64};
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;

use super::{
    AUX_PC_CHANNEL_BASE, AUX_PC_MULT, AUX_PC_TABLE, CHALLENGE_POPCOUNT_DENOM,
    CHALLENGE_POPCOUNT_FOLD,
};
use crate::bus::BusSlot;
use crate::{AirError, Felt, QuadExt, Result};

/// Entries in the popcount table: one per byte value.
const TABLE_SIZE: usize = 256;

const POPCNT: [OpCode; 2] = [OpCode::I32Popcnt, OpCode::I64Popcnt];

pub(crate) fn evaluate<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB) {
    let main = builder.main();
    let local = main.current_slice();

    value_relations(builder, local);

    let fold: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_POPCOUNT_FOLD].into();
    let denom: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_POPCOUNT_DENOM].into();
    // The byte side reuses the range check's identity byte-table column (index 1).
    let table = fold_pair::<AB>(
        fold.dup(),
        builder.periodic_values()[1].into(),
        builder.periodic_values()[8].into(),
    );

    let perm = builder.permutation();
    for i in 0..POPCNT_BYTES {
        let channel = fold_pair::<AB>(
            fold.dup(),
            local[POPCNT_BYTE_BASE + i].into(),
            local[POPCNT_PC_BASE + i].into(),
        );
        let s_cur = perm.current_slice()[AUX_PC_CHANNEL_BASE + i];
        let s_next: AB::ExprEF = perm.next_slice()[AUX_PC_CHANNEL_BASE + i].into();
        let s_delta: AB::ExprEF = s_next - Into::<AB::ExprEF>::into(s_cur);
        builder
            .when_transition()
            .assert_zero_ext(s_delta * (denom.dup() - channel) - AB::ExprEF::ONE);
    }

    let m_cur = perm.current_slice()[AUX_PC_MULT];
    let sm_cur = perm.current_slice()[AUX_PC_TABLE];
    let sm_next: AB::ExprEF = perm.next_slice()[AUX_PC_TABLE].into();
    let sm_delta: AB::ExprEF = sm_next - Into::<AB::ExprEF>::into(sm_cur);
    builder
        .when_transition()
        .assert_zero_ext(sm_delta * (denom.dup() - table) - Into::<AB::ExprEF>::into(m_cur));

    for i in 0..POPCNT_BYTES {
        builder
            .when_first_row()
            .assert_zero_ext(perm.current_slice()[AUX_PC_CHANNEL_BASE + i]);
    }
    builder.when_first_row().assert_zero_ext(sm_cur);
    let sum_channels = (0..POPCNT_BYTES).fold(AB::ExprEF::ZERO, |acc, i| {
        acc + Into::<AB::ExprEF>::into(perm.current_slice()[AUX_PC_CHANNEL_BASE + i])
    });
    builder
        .when_last_row()
        .assert_zero_ext(sum_channels - Into::<AB::ExprEF>::into(sm_cur));
}

fn value_relations<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB, local: &[AB::Var]) {
    let selector: AB::Expr = POPCNT.iter().fold(AB::Expr::ZERO, |acc, &op| {
        acc + local[SELECTOR_BASE + opcode_index(op)]
    });
    let limb = |start: usize| -> AB::Expr {
        (0..LIMB_BYTES).fold(AB::Expr::ZERO, |acc, k| {
            acc + local[POPCNT_BYTE_BASE + start + k] * Felt::new(1u64 << (8 * k))
        })
    };
    let count = (0..POPCNT_BYTES).fold(AB::Expr::ZERO, |acc, i| acc + local[POPCNT_PC_BASE + i]);

    let s0 = BusSlot::read(local, 0);
    let s1 = BusSlot::read(local, 1);
    // The bytes decompose the operand; the result is the sum of their popcounts.
    builder.assert_zero(selector.dup() * (s0.lo - limb(0)));
    builder.assert_zero(selector.dup() * (s0.hi - limb(LIMB_BYTES)));
    builder.assert_zero(selector.dup() * (s1.lo - count));
    builder.assert_zero(selector * s1.hi);
}

/// Folds a `(byte, popcount)` pair into `byte + popcount * fold`.
fn fold_pair<AB: ExtensionBuilder<F = Felt>>(
    fold: AB::ExprEF,
    byte: AB::Expr,
    popcount: AB::Expr,
) -> AB::ExprEF {
    fold * popcount + byte
}

pub fn table(length: usize) -> Vec<Felt> {
    (0..length)
        .map(|row| {
            Felt::new(if row < TABLE_SIZE {
                u64::from((row as u32).count_ones())
            } else {
                0
            })
        })
        .collect()
}

pub(crate) fn columns(
    main: &RowMajorMatrix<Felt>,
    fold: QuadExt,
    denom: QuadExt,
) -> Result<Vec<Vec<QuadExt>>> {
    let height = main.height();
    let width = main.width();
    let row = |r: usize| &main.values[r * width..(r + 1) * width];
    let summed = height.saturating_sub(1);

    let mut counts = vec![0u64; TABLE_SIZE];
    for r in 0..summed {
        for i in 0..POPCNT_BYTES {
            counts[table_index(row(r), i, r)?] += 1;
        }
    }

    let fold_row =
        |byte: Felt, popcount: Felt| QuadExt::from(popcount) * fold + QuadExt::from(byte);
    let reciprocal = |value: QuadExt| value.try_inverse().ok_or(AirError::DegenerateChallenge);
    let table_at = |i: usize| -> (Felt, Felt) {
        if i < TABLE_SIZE {
            (
                Felt::new(i as u64),
                Felt::new(u64::from((i as u32).count_ones())),
            )
        } else {
            (Felt::ZERO, Felt::ZERO)
        }
    };

    let mut multiplicity = vec![QuadExt::ZERO; height];
    for (value, &count) in counts.iter().enumerate() {
        multiplicity[value] = QuadExt::from(Felt::new(count));
    }

    let mut table_sum = vec![QuadExt::ZERO; height];
    for i in 0..summed {
        let (byte, popcount) = table_at(i);
        let step = multiplicity[i] * reciprocal(denom - fold_row(byte, popcount))?;
        table_sum[i + 1] = table_sum[i] + step;
    }

    let mut channels: Vec<Vec<QuadExt>> = vec![vec![QuadExt::ZERO; height]; POPCNT_BYTES];
    for (i, accumulator) in channels.iter_mut().enumerate() {
        for r in 0..summed {
            let (byte, popcount) = (row(r)[POPCNT_BYTE_BASE + i], row(r)[POPCNT_PC_BASE + i]);
            accumulator[r + 1] = accumulator[r] + reciprocal(denom - fold_row(byte, popcount))?;
        }
    }

    Ok(once(multiplicity)
        .chain(once(table_sum))
        .chain(channels)
        .collect())
}

fn table_index(row: &[Felt], byte: usize, r: usize) -> Result<usize> {
    let value = usize::try_from(row[POPCNT_BYTE_BASE + byte].as_canonical_u64())
        .ok()
        .filter(|&v| v < TABLE_SIZE)
        .ok_or_else(|| {
            AirError::LookupBuild(format!("row {r} popcount byte {byte} is not a byte"))
        })?;
    let popcount = row[POPCNT_PC_BASE + byte].as_canonical_u64();
    if popcount != u64::from((value as u32).count_ones()) {
        return Err(AirError::LookupBuild(format!(
            "row {r} popcount byte {byte} is not a population count"
        )));
    }
    Ok(value)
}

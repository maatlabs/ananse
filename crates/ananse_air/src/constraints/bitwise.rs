//! Bitwise `and`/`or`/`xor` value relations, resolved through a nibble AND-table
//! LogUp lookup.
//!
//! Each operand splits into sixteen four-bit nibbles. A logderivative lookup against
//! a `16 x 16` table proves every nibble triple `(a, b, a & b)` is a genuine bitwise
//! AND, which simultaneously pins each input nibble into `[0, 16)`. The decomposition
//! constraints tie those nibbles to the operands on the bus, and the two other
//! operators follow arithmetically per nibble with no inter-nibble carries: `a | b =
//! a + b - (a & b)` and `a ^ b = a + b - 2 (a & b)`.

use std::iter::once;

use ananse_executor::OpCode;
use ananse_trace::layout::{BW_A_BASE, BW_B_BASE, BW_NIBBLES, BW_P_BASE, SELECTOR_BASE};
use ananse_trace::selector::opcode_index;
use p3_air::{ExtensionBuilder, PermutationAirBuilder, WindowAccess};
use p3_field::{Dup, Field, PrimeCharacteristicRing, PrimeField64};
use p3_goldilocks::Goldilocks as Felt;
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;

use crate::bus::BusSlot;
use crate::{
    AUX_BW_CHANNEL_BASE, AUX_BW_MULT, AUX_BW_TABLE, AirError, CHALLENGE_BITWISE_DENOM,
    CHALLENGE_BITWISE_FOLD, Ext, Result,
};

/// Entries in the nibble AND-table: every ordered pair of four-bit values.
const AND_TABLE_SIZE: usize = 256;

/// Nibbles decomposing one 32-bit limb.
const LIMB_NIBBLES: usize = BW_NIBBLES / 2;

/// The six binary bitwise operators.
pub(crate) const BINARY: [OpCode; 6] = [
    OpCode::I32And,
    OpCode::I32Or,
    OpCode::I32Xor,
    OpCode::I64And,
    OpCode::I64Or,
    OpCode::I64Xor,
];

pub(crate) fn evaluate<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB) {
    let main = builder.main();
    let local = main.current_slice();

    value_relations(builder, local);

    let fold: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_BITWISE_FOLD].into();
    let denom: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_BITWISE_DENOM].into();
    let table = fold_tuple::<AB>(
        fold.dup(),
        [
            builder.periodic_values()[5].into(),
            builder.periodic_values()[6].into(),
            builder.periodic_values()[7].into(),
        ],
    );

    let perm = builder.permutation();

    // Each nibble position's running reciprocal advances by `1 / (denom - tuple)`.
    for i in 0..BW_NIBBLES {
        let tuple = fold_tuple::<AB>(
            fold.dup(),
            [
                local[BW_A_BASE + i].into(),
                local[BW_B_BASE + i].into(),
                local[BW_P_BASE + i].into(),
            ],
        );
        let s_cur = perm.current_slice()[AUX_BW_CHANNEL_BASE + i];
        let s_next: AB::ExprEF = perm.next_slice()[AUX_BW_CHANNEL_BASE + i].into();
        let s_delta: AB::ExprEF = s_next - Into::<AB::ExprEF>::into(s_cur);
        builder
            .when_transition()
            .assert_zero_ext(s_delta * (denom.dup() - tuple) - AB::ExprEF::ONE);
    }

    // The table side advances by `m_v / (denom - v)`.
    let m_cur = perm.current_slice()[AUX_BW_MULT];
    let sm_cur = perm.current_slice()[AUX_BW_TABLE];
    let sm_next: AB::ExprEF = perm.next_slice()[AUX_BW_TABLE].into();
    let sm_delta: AB::ExprEF = sm_next - Into::<AB::ExprEF>::into(sm_cur);
    builder
        .when_transition()
        .assert_zero_ext(sm_delta * (denom.dup() - table) - Into::<AB::ExprEF>::into(m_cur));

    // Every accumulator opens at zero and the balance closes at zero, so every nibble
    // triple is a table member: a genuine bitwise AND of two four-bit values.
    for i in 0..BW_NIBBLES {
        builder
            .when_first_row()
            .assert_zero_ext(perm.current_slice()[AUX_BW_CHANNEL_BASE + i]);
    }
    builder.when_first_row().assert_zero_ext(sm_cur);
    let sum_channels = (0..BW_NIBBLES).fold(AB::ExprEF::ZERO, |acc, i| {
        acc + Into::<AB::ExprEF>::into(perm.current_slice()[AUX_BW_CHANNEL_BASE + i])
    });
    builder
        .when_last_row()
        .assert_zero_ext(sum_channels - Into::<AB::ExprEF>::into(sm_cur));
}

fn value_relations<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB, local: &[AB::Var]) {
    let sum = |ops: &[OpCode]| -> AB::Expr {
        ops.iter().fold(AB::Expr::ZERO, |acc, &op| {
            acc + local[SELECTOR_BASE + opcode_index(op)]
        })
    };
    let limb = |base: usize, start: usize| -> AB::Expr {
        (0..LIMB_NIBBLES).fold(AB::Expr::ZERO, |acc, k| {
            acc + local[base + start + k] * Felt::new(1u64 << (4 * k))
        })
    };

    let c2 = BusSlot::read(local, 0);
    let c1 = BusSlot::read(local, 1);
    let r = BusSlot::read(local, 2);

    // The nibbles decompose the operands the bus carries.
    builder.assert_zero(sum(&BINARY) * (c1.lo - limb(BW_A_BASE, 0)));
    builder.assert_zero(sum(&BINARY) * (c1.hi - limb(BW_A_BASE, LIMB_NIBBLES)));
    builder.assert_zero(sum(&BINARY) * (c2.lo - limb(BW_B_BASE, 0)));
    builder.assert_zero(sum(&BINARY) * (c2.hi - limb(BW_B_BASE, LIMB_NIBBLES)));

    // `and` is the looked-up product; `or` and `xor` follow per nibble.
    let and = [OpCode::I32And, OpCode::I64And];
    let or = [OpCode::I32Or, OpCode::I64Or];
    let xor = [OpCode::I32Xor, OpCode::I64Xor];
    builder.assert_zero(sum(&and) * (r.lo - limb(BW_P_BASE, 0)));
    builder.assert_zero(sum(&and) * (r.hi - limb(BW_P_BASE, LIMB_NIBBLES)));
    builder.assert_zero(sum(&or) * (r.lo - c1.lo - c2.lo + limb(BW_P_BASE, 0)));
    builder.assert_zero(sum(&or) * (r.hi - c1.hi - c2.hi + limb(BW_P_BASE, LIMB_NIBBLES)));
    builder.assert_zero(sum(&xor) * (r.lo - c1.lo - c2.lo + limb(BW_P_BASE, 0) * Felt::new(2)));
    builder.assert_zero(
        sum(&xor) * (r.hi - c1.hi - c2.hi + limb(BW_P_BASE, LIMB_NIBBLES) * Felt::new(2)),
    );
}

/// Folds a nibble triple `(a, b, p)` into `a + b*fold + p*fold^2`.
fn fold_tuple<AB: ExtensionBuilder<F = Felt>>(
    fold: AB::ExprEF,
    terms: [AB::Expr; 3],
) -> AB::ExprEF {
    terms
        .into_iter()
        .rev()
        .fold(AB::ExprEF::ZERO, |acc, term| acc * fold.dup() + term)
}

pub fn and_table(length: usize) -> [Vec<Felt>; 3] {
    let column = |select: fn(usize) -> u64| -> Vec<Felt> {
        (0..length)
            .map(|row| Felt::new(if row < AND_TABLE_SIZE { select(row) } else { 0 }))
            .collect()
    };
    [
        column(|v| (v >> 4) as u64),
        column(|v| (v & 0xf) as u64),
        column(|v| ((v >> 4) & v & 0xf) as u64),
    ]
}

pub(crate) fn columns(main: &RowMajorMatrix<Felt>, fold: Ext, denom: Ext) -> Result<Vec<Vec<Ext>>> {
    let height = main.height();
    let width = main.width();
    let row = |r: usize| &main.values[r * width..(r + 1) * width];
    let summed = height.saturating_sub(1);

    let mut counts = vec![0u64; AND_TABLE_SIZE];
    for r in 0..summed {
        for i in 0..BW_NIBBLES {
            counts[table_index(row(r), i, r)?] += 1;
        }
    }

    let fold_row = |a: Felt, b: Felt, p: Felt| {
        [a, b, p]
            .into_iter()
            .rev()
            .fold(Ext::ZERO, |acc, term| acc * fold + Ext::from(term))
    };
    let reciprocal = |value: Ext| value.try_inverse().ok_or(AirError::DegenerateChallenge);
    let table_at = |i: usize| -> (Felt, Felt, Felt) {
        let (a, b) = if i < AND_TABLE_SIZE {
            ((i >> 4) as u64, (i & 0xf) as u64)
        } else {
            (0, 0)
        };
        (Felt::new(a), Felt::new(b), Felt::new(a & b))
    };

    let mut multiplicity = vec![Ext::ZERO; height];
    for (value, &count) in counts.iter().enumerate() {
        multiplicity[value] = Ext::from(Felt::new(count));
    }

    let mut table_sum = vec![Ext::ZERO; height];
    for i in 0..summed {
        let (a, b, p) = table_at(i);
        let step = multiplicity[i] * reciprocal(denom - fold_row(a, b, p))?;
        table_sum[i + 1] = table_sum[i] + step;
    }

    let mut channels: Vec<Vec<Ext>> = vec![vec![Ext::ZERO; height]; BW_NIBBLES];
    for (i, accumulator) in channels.iter_mut().enumerate() {
        for r in 0..summed {
            let (a, b, p) = (
                row(r)[BW_A_BASE + i],
                row(r)[BW_B_BASE + i],
                row(r)[BW_P_BASE + i],
            );
            accumulator[r + 1] = accumulator[r] + reciprocal(denom - fold_row(a, b, p))?;
        }
    }

    Ok(once(multiplicity)
        .chain(once(table_sum))
        .chain(channels)
        .collect())
}

fn table_index(row: &[Felt], nibble: usize, r: usize) -> Result<usize> {
    let nibble_of = |base: usize| -> Result<usize> {
        usize::try_from(row[base + nibble].as_canonical_u64())
            .ok()
            .filter(|&value| value < 16)
            .ok_or_else(|| {
                AirError::LookupBuild(format!("row {r} nibble {nibble} is not a 4-bit value"))
            })
    };
    let a = nibble_of(BW_A_BASE)?;
    let b = nibble_of(BW_B_BASE)?;
    let p = usize::try_from(row[BW_P_BASE + nibble].as_canonical_u64()).unwrap_or(usize::MAX);
    if p != (a & b) {
        return Err(AirError::LookupBuild(format!(
            "row {r} nibble {nibble} is not a bitwise AND"
        )));
    }
    Ok((a << 4) | b)
}

//! Boundary initialization: a gated logderivative lookup binding every fresh read to
//! the frame's public initial state.

use std::collections::HashMap;

use ananse_trace::layout::BUS_SLOTS;
use p3_air::{ExtensionBuilder, PermutationAirBuilder, WindowAccess};
use p3_field::{Dup, Field, PrimeCharacteristicRing, PrimeField64};
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;

use super::{
    AUX_BND_CHANNEL_BASE, AUX_BND_MULT, AUX_BND_TABLE, CHALLENGE_BOUNDARY, CHALLENGE_FOLD,
};
use crate::bus::SortedEntry;
use crate::{AirError, Felt, QuadExt};

pub(crate) fn evaluate<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB) {
    let main = builder.main();
    let local = main.current_slice();
    let fold: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_FOLD].into();
    let gamma: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_BOUNDARY].into();

    // The table entry rides three periodic columns, folded into one element.
    let addr_e: AB::Expr = builder.periodic_values()[2].into();
    let lo_e: AB::Expr = builder.periodic_values()[3].into();
    let hi_e: AB::Expr = builder.periodic_values()[4].into();
    let fold_table = fold_triple::<AB>(fold.dup(), addr_e, lo_e, hi_e);

    let perm = builder.permutation();

    // The table side advances by `m / (gamma - fold(table entry))`.
    let m_cur = perm.current_slice()[AUX_BND_MULT];
    let sm_cur = perm.current_slice()[AUX_BND_TABLE];
    let sm_next: AB::ExprEF = perm.next_slice()[AUX_BND_TABLE].into();
    let sm_delta: AB::ExprEF = sm_next - Into::<AB::ExprEF>::into(sm_cur);
    builder
        .when_transition()
        .assert_zero_ext(sm_delta * (gamma.dup() - fold_table) - Into::<AB::ExprEF>::into(m_cur));

    // Each channel's running reciprocal advances by `gate / (gamma - fold(entry))`,
    // where the gate selects the fresh reads.
    for k in 0..BUS_SLOTS {
        let entry = SortedEntry::read(local, k);
        let fold_entry = fold_triple::<AB>(
            fold.dup(),
            entry.addr.into(),
            entry.lo.into(),
            entry.hi.into(),
        );
        let active: AB::Expr = entry.active.into();
        let gate: AB::Expr =
            active * (AB::Expr::ONE - entry.same_addr) * (AB::Expr::ONE - entry.is_write);
        let s_cur = perm.current_slice()[AUX_BND_CHANNEL_BASE + k];
        let s_next: AB::ExprEF = perm.next_slice()[AUX_BND_CHANNEL_BASE + k].into();
        let s_delta: AB::ExprEF = s_next - Into::<AB::ExprEF>::into(s_cur);
        builder
            .when_transition()
            .assert_zero_ext(s_delta * (gamma.dup() - fold_entry) - Into::<AB::ExprEF>::into(gate));
    }

    // Every accumulator opens at zero; the balance closes at zero, so every fresh read
    // is a member of the initial-state table with the declared value.
    for k in 0..BUS_SLOTS {
        builder
            .when_first_row()
            .assert_zero_ext(perm.current_slice()[AUX_BND_CHANNEL_BASE + k]);
    }
    builder.when_first_row().assert_zero_ext(sm_cur);
    let sum_channels = (0..BUS_SLOTS).fold(AB::ExprEF::ZERO, |acc, k| {
        acc + Into::<AB::ExprEF>::into(perm.current_slice()[AUX_BND_CHANNEL_BASE + k])
    });
    builder
        .when_last_row()
        .assert_zero_ext(sum_channels - Into::<AB::ExprEF>::into(sm_cur));
}

pub(crate) fn periodic_columns(table: &[(u64, Felt, Felt)], length: usize) -> [Vec<Felt>; 3] {
    let column = |select: fn((u64, Felt, Felt)) -> Felt| -> Vec<Felt> {
        (0..length)
            .map(|row| table.get(row).copied().map_or(Felt::ZERO, select))
            .collect()
    };
    [
        column(|(addr, _, _)| Felt::new(addr)),
        column(|(_, lo, _)| lo),
        column(|(_, _, hi)| hi),
    ]
}

pub(crate) fn columns(
    main: &RowMajorMatrix<Felt>,
    table: &[(u64, Felt, Felt)],
    fold: QuadExt,
    denom: QuadExt,
) -> Result<Vec<Vec<QuadExt>>, AirError> {
    let height = main.height();
    let width = main.width();
    let row = |r: usize| &main.values[r * width..(r + 1) * width];
    // The terminal row is excluded: the accumulators never reach it, so its fresh
    // reads and table entries are neither summed nor counted.
    let summed = height.saturating_sub(1);

    if table.len() > summed {
        return Err(AirError::TraceTooShortForBoundary {
            trace_len: height,
            table_len: table.len(),
        });
    }

    let folded = |addr: Felt, lo: Felt, hi: Felt| -> QuadExt {
        QuadExt::from(addr) + fold * QuadExt::from(lo) + fold * fold * QuadExt::from(hi)
    };
    let reciprocal = |denominator: QuadExt| {
        denominator
            .try_inverse()
            .ok_or(AirError::DegenerateChallenge)
    };

    let index: HashMap<(u64, u64, u64), usize> = table
        .iter()
        .enumerate()
        .map(|(i, &(addr, lo, hi))| ((addr, lo.as_canonical_u64(), hi.as_canonical_u64()), i))
        .collect();

    // Tally each fresh read against the initial-state entry it opens; a read whose
    // value is absent from the table has no valid multiplicity.
    let mut counts = vec![0u64; table.len()];
    for r in 0..summed {
        for k in 0..BUS_SLOTS {
            let entry = SortedEntry::read(row(r), k);
            if is_fresh_read(entry) {
                let key = (
                    entry.addr.as_canonical_u64(),
                    entry.lo.as_canonical_u64(),
                    entry.hi.as_canonical_u64(),
                );
                let position = index.get(&key).ok_or_else(|| {
                    AirError::LookupBuild(format!(
                        "fresh read at row {r} entry {k} is absent from the initial-state table"
                    ))
                })?;
                counts[*position] = counts[*position].saturating_add(1);
            }
        }
    }

    let table_at = |i: usize| table.get(i).copied().unwrap_or((0, Felt::ZERO, Felt::ZERO));

    let mut multiplicity = vec![QuadExt::ZERO; height];
    for (i, &count) in counts.iter().enumerate() {
        multiplicity[i] = QuadExt::from(Felt::new(count));
    }

    let mut table_sum = vec![QuadExt::ZERO; height];
    for i in 0..summed {
        let step = if multiplicity[i] == QuadExt::ZERO {
            QuadExt::ZERO
        } else {
            let (addr, lo, hi) = table_at(i);
            multiplicity[i] * reciprocal(denom - folded(Felt::new(addr), lo, hi))?
        };
        table_sum[i + 1] = table_sum[i] + step;
    }

    let mut channels: Vec<Vec<QuadExt>> = vec![vec![QuadExt::ZERO; height]; BUS_SLOTS];
    for (k, accumulator) in channels.iter_mut().enumerate() {
        for i in 0..summed {
            let entry = SortedEntry::read(row(i), k);
            let step = if is_fresh_read(entry) {
                reciprocal(denom - folded(entry.addr, entry.lo, entry.hi))?
            } else {
                QuadExt::ZERO
            };
            accumulator[i + 1] = accumulator[i] + step;
        }
    }

    Ok(std::iter::once(multiplicity)
        .chain(std::iter::once(table_sum))
        .chain(channels)
        .collect())
}

fn is_fresh_read(entry: SortedEntry<Felt>) -> bool {
    entry.active == Felt::ONE && entry.same_addr == Felt::ZERO && entry.is_write == Felt::ZERO
}

fn fold_triple<AB: ExtensionBuilder<F = Felt>>(
    challenge: AB::ExprEF,
    addr: AB::Expr,
    lo: AB::Expr,
    hi: AB::Expr,
) -> AB::ExprEF {
    [addr, lo, hi]
        .into_iter()
        .rev()
        .fold(AB::ExprEF::ZERO, |acc, term| acc * challenge.dup() + term)
}

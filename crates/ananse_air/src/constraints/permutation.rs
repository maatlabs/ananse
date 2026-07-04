//! Bus-to-sorted-log consistency permutation: a logderivative multiset-equality argument.

use ananse_trace::layout::{BUS_SLOTS, COL_CLK};
use p3_air::{ExtensionBuilder, PermutationAirBuilder, WindowAccess};
use p3_field::{Dup, Field, PrimeCharacteristicRing};
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;

use super::{AUX_BUS_ACC, AUX_SORTED_ACC, CHALLENGE_DENOM, CHALLENGE_FOLD};
use crate::bus::{BusSlot, SortedEntry};
use crate::{AirError, Felt, QuadExt, Result};

pub(crate) fn evaluate<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB) {
    let main = builder.main();
    let local = main.current_slice();
    let clk = local[COL_CLK];

    let denom: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_DENOM].into();
    let fold: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_FOLD].into();

    let bus_denoms: Vec<AB::ExprEF> = (0..BUS_SLOTS)
        .map(|s| {
            let slot = BusSlot::read(local, s);
            let ts = clk * Felt::new(BUS_SLOTS as u64) + Felt::new(s as u64);
            let folded = fold_expr::<AB>(
                fold.dup(),
                [
                    slot.addr.into(),
                    ts,
                    slot.lo.into(),
                    slot.hi.into(),
                    slot.is_write.into(),
                ],
            );
            denom.dup() - folded
        })
        .collect();
    let bus_actives: Vec<AB::Expr> = (0..BUS_SLOTS)
        .map(|s| BusSlot::read(local, s).active.into())
        .collect();

    let sorted_denoms: Vec<AB::ExprEF> = (0..BUS_SLOTS)
        .map(|e| {
            let entry = SortedEntry::read(local, e);
            let folded = fold_expr::<AB>(
                fold.dup(),
                [
                    entry.addr.into(),
                    entry.ts.into(),
                    entry.lo.into(),
                    entry.hi.into(),
                    entry.is_write.into(),
                ],
            );
            denom.dup() - folded
        })
        .collect();
    let sorted_actives: Vec<AB::Expr> = (0..BUS_SLOTS)
        .map(|e| SortedEntry::read(local, e).active.into())
        .collect();

    let perm = builder.permutation();
    let bus_cur = perm.current_slice()[AUX_BUS_ACC];
    let bus_next = perm.next_slice()[AUX_BUS_ACC];
    let sorted_cur = perm.current_slice()[AUX_SORTED_ACC];
    let sorted_next = perm.next_slice()[AUX_SORTED_ACC];

    let bus_delta: AB::ExprEF = bus_next.into() - Into::<AB::ExprEF>::into(bus_cur);
    let sorted_delta: AB::ExprEF = sorted_next.into() - Into::<AB::ExprEF>::into(sorted_cur);

    accumulate(builder, bus_delta, &bus_denoms, &bus_actives);
    accumulate(builder, sorted_delta, &sorted_denoms, &sorted_actives);

    builder.when_first_row().assert_zero_ext(bus_cur);
    builder.when_first_row().assert_zero_ext(sorted_cur);
    let balance: AB::ExprEF = bus_cur.into() - Into::<AB::ExprEF>::into(sorted_cur);
    builder.when_last_row().assert_zero_ext(balance);
}

pub(crate) fn consistency_columns(
    main: &RowMajorMatrix<Felt>,
    denom: QuadExt,
    fold: QuadExt,
) -> Result<(Vec<QuadExt>, Vec<QuadExt>)> {
    let height = main.height();
    let width = main.width();
    let row = |r: usize| &main.values[r * width..(r + 1) * width];

    let mut bus_acc = vec![QuadExt::ZERO; height];
    let mut sorted_acc = vec![QuadExt::ZERO; height];

    for r in 0..height.saturating_sub(1) {
        bus_acc[r + 1] = bus_acc[r] + bus_contribution(row(r), denom, fold)?;
        sorted_acc[r + 1] = sorted_acc[r] + sorted_contribution(row(r), denom, fold)?;
    }
    Ok((bus_acc, sorted_acc))
}

fn bus_contribution(row: &[Felt], denom: QuadExt, fold: QuadExt) -> Result<QuadExt> {
    let clk = row[COL_CLK];
    (0..BUS_SLOTS).try_fold(QuadExt::ZERO, |acc, s| {
        let slot = BusSlot::read(row, s);
        if slot.active == Felt::ZERO {
            return Ok(acc);
        }
        let ts = clk * Felt::new(BUS_SLOTS as u64) + Felt::new(s as u64);
        let folded = fold_access(fold, slot.addr, ts, slot.lo, slot.hi, slot.is_write);
        reciprocal(denom, folded).map(|inv| acc + inv)
    })
}

fn sorted_contribution(row: &[Felt], denom: QuadExt, fold: QuadExt) -> Result<QuadExt> {
    (0..BUS_SLOTS).try_fold(QuadExt::ZERO, |acc, e| {
        let entry = SortedEntry::read(row, e);
        if entry.active == Felt::ZERO {
            return Ok(acc);
        }
        let folded = fold_access(
            fold,
            entry.addr,
            entry.ts,
            entry.lo,
            entry.hi,
            entry.is_write,
        );
        reciprocal(denom, folded).map(|inv| acc + inv)
    })
}

fn fold_access(
    challenge: QuadExt,
    addr: Felt,
    ts: Felt,
    lo: Felt,
    hi: Felt,
    is_write: Felt,
) -> QuadExt {
    [addr, ts, lo, hi, is_write]
        .into_iter()
        .rev()
        .fold(QuadExt::ZERO, |acc, term| {
            acc * challenge + QuadExt::from(term)
        })
}

fn reciprocal(denom: QuadExt, folded: QuadExt) -> Result<QuadExt> {
    (denom - folded)
        .try_inverse()
        .ok_or(AirError::DegenerateChallenge)
}

fn accumulate<AB: PermutationAirBuilder<F = Felt>>(
    builder: &mut AB,
    delta: AB::ExprEF,
    denoms: &[AB::ExprEF],
    actives: &[AB::Expr],
) {
    let full = denoms.iter().fold(AB::ExprEF::ONE, |acc, d| acc * d.dup());
    let numerator = (0..denoms.len())
        .map(|s| {
            let omit = denoms
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != s)
                .fold(AB::ExprEF::ONE, |acc, (_, d)| acc * d.dup());
            omit * actives[s].dup()
        })
        .fold(AB::ExprEF::ZERO, |acc, term| acc + term);
    builder
        .when_transition()
        .assert_zero_ext(delta * full - numerator);
}

fn fold_expr<AB: ExtensionBuilder<F = Felt>>(
    challenge: AB::ExprEF,
    terms: [AB::Expr; 5],
) -> AB::ExprEF {
    terms
        .into_iter()
        .rev()
        .fold(AB::ExprEF::ZERO, |acc, term| acc * challenge.dup() + term)
}

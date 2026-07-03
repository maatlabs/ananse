//! Native range checks: an 8-bit byte-table lookup proving the sortedness ordering
//! and the written-value encoding's injectivity.

use ananse_trace::layout::{
    BUS_SLOTS, GAP_BYTES, LIMB_BYTES, RANGE_BASE, RANGE_COLS, RANGE_TABLE_SIZE, RC_WRITE_HI,
    RC_WRITE_LO, rc_gap,
};
use p3_air::{AirBuilder, ExtensionBuilder, PermutationAirBuilder, WindowAccess};
use p3_field::{Dup, Field, PrimeCharacteristicRing, PrimeField64};
use p3_goldilocks::Goldilocks as Felt;
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;

use crate::bus::{BusSlot, SortedEntry};
use crate::{
    AUX_RC_CHANNEL_BASE, AUX_RC_MULT, AUX_RC_TABLE, AirError, CHALLENGE_RANGE, Ext, Result,
};

/// Evaluates the range-check family: the written-value and ordering-gap
/// byte-decomposition identities on the main trace, and the byte-table LogUp binding
/// every decomposition byte to the table.
pub(crate) fn evaluate<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB) {
    let main = builder.main();
    let local = main.current_slice();
    let next = main.next_slice();
    let beta: AB::ExprEF = builder.permutation_randomness()[CHALLENGE_RANGE].into();
    let table: AB::Expr = builder.periodic_values()[1].into();

    // The single written value's two limbs, selected off the value bus, equal their
    // byte decompositions.
    let (write_lo, write_hi) = written_value::<AB>(local);
    builder.assert_zero(write_lo - decompose::<AB>(local, RC_WRITE_LO, LIMB_BYTES));
    builder.assert_zero(write_hi - decompose::<AB>(local, RC_WRITE_HI, LIMB_BYTES));

    // Each active sorted pair's ordering gap equals its byte decomposition.
    for pair in 0..BUS_SLOTS {
        let prev = SortedEntry::read(local, pair);
        let cur = if pair + 1 < BUS_SLOTS {
            SortedEntry::read(local, pair + 1)
        } else {
            SortedEntry::read(next, 0)
        };
        let ts_gap: AB::Expr = cur.ts - prev.ts;
        let addr_gap: AB::Expr = cur.addr - prev.addr;
        let selected: AB::Expr =
            cur.same_addr * ts_gap + (AB::Expr::ONE - cur.same_addr) * addr_gap;
        let gap = selected - AB::Expr::ONE - decompose::<AB>(local, rc_gap(pair), GAP_BYTES);
        builder.when_transition().assert_zero(cur.active * gap);
    }

    let perm = builder.permutation();

    // Each channel's running reciprocal advances by `1 / (beta - byte)`.
    for channel in 0..RANGE_COLS {
        let byte: AB::Expr = local[RANGE_BASE + channel].into();
        let s_cur = perm.current_slice()[AUX_RC_CHANNEL_BASE + channel];
        let s_next: AB::ExprEF = perm.next_slice()[AUX_RC_CHANNEL_BASE + channel].into();
        let s_delta: AB::ExprEF = s_next - Into::<AB::ExprEF>::into(s_cur);
        builder
            .when_transition()
            .assert_zero_ext(s_delta * (beta.dup() - byte) - AB::ExprEF::ONE);
    }

    // The table side advances by `m_v / (beta - v)`.
    let m_cur = perm.current_slice()[AUX_RC_MULT];
    let sm_cur = perm.current_slice()[AUX_RC_TABLE];
    let sm_next: AB::ExprEF = perm.next_slice()[AUX_RC_TABLE].into();
    let sm_delta: AB::ExprEF = sm_next - Into::<AB::ExprEF>::into(sm_cur);
    builder
        .when_transition()
        .assert_zero_ext(sm_delta * (beta.dup() - table) - Into::<AB::ExprEF>::into(m_cur));

    // Every accumulator opens at zero; the balance closes at zero, so every
    // decomposition byte is a member of the table.
    for channel in 0..RANGE_COLS {
        builder
            .when_first_row()
            .assert_zero_ext(perm.current_slice()[AUX_RC_CHANNEL_BASE + channel]);
    }
    builder.when_first_row().assert_zero_ext(sm_cur);
    let sum_channels = (0..RANGE_COLS).fold(AB::ExprEF::ZERO, |acc, channel| {
        acc + Into::<AB::ExprEF>::into(perm.current_slice()[AUX_RC_CHANNEL_BASE + channel])
    });
    builder
        .when_last_row()
        .assert_zero_ext(sum_channels - Into::<AB::ExprEF>::into(sm_cur));
}

/// The verifier-filled periodic column carrying the 8-bit byte table: row `i` holds
/// the byte value `i` for `i < 256`, and zero beyond, where the multiplicity is zero.
/// `length` is the padded trace height.
pub fn byte_table(length: usize) -> Vec<Felt> {
    (0..length)
        .map(|row| {
            Felt::new(if row < RANGE_TABLE_SIZE {
                row as u64
            } else {
                0
            })
        })
        .collect()
}

/// Builds the byte-table LogUp columns in permutation-trace order: the per-value
/// multiplicity, the table-side running reciprocal, and one running reciprocal per
/// byte channel (one channel per range-check witness column), all folded by `beta`.
pub(crate) fn columns(main: &RowMajorMatrix<Felt>, beta: Ext) -> Result<Vec<Vec<Ext>>> {
    let height = main.height();
    let width = main.width();
    let row = |r: usize| &main.values[r * width..(r + 1) * width];
    // The terminal row is excluded: the accumulators never reach it, so its bytes are
    // neither summed nor counted.
    let summed = height.saturating_sub(1);

    let mut counts = vec![0u64; RANGE_TABLE_SIZE];
    for r in 0..summed {
        for channel in 0..RANGE_COLS {
            let byte = row(r)[RANGE_BASE + channel].as_canonical_u64();
            let slot = usize::try_from(byte)
                .ok()
                .filter(|&v| v < RANGE_TABLE_SIZE)
                .ok_or_else(|| {
                    AirError::LookupBuild(format!(
                        "row {r} range channel {channel} holds {byte}, not an 8-bit byte"
                    ))
                })?;
            counts[slot] = counts[slot].saturating_add(1);
        }
    }

    let table_at = |i: usize| Felt::new(if i < RANGE_TABLE_SIZE { i as u64 } else { 0 });
    let reciprocal = |denominator: Ext| {
        denominator
            .try_inverse()
            .ok_or(AirError::DegenerateChallenge)
    };

    let mut multiplicity = vec![Ext::ZERO; height];
    for (value, &count) in counts.iter().enumerate() {
        multiplicity[value] = Ext::from(Felt::new(count));
    }

    let mut table_sum = vec![Ext::ZERO; height];
    for i in 0..summed {
        let step = multiplicity[i] * reciprocal(beta - Ext::from(table_at(i)))?;
        table_sum[i + 1] = table_sum[i] + step;
    }

    let mut channels: Vec<Vec<Ext>> = vec![vec![Ext::ZERO; height]; RANGE_COLS];
    for (channel, accumulator) in channels.iter_mut().enumerate() {
        for i in 0..summed {
            let byte = row(i)[RANGE_BASE + channel];
            accumulator[i + 1] = accumulator[i] + reciprocal(beta - Ext::from(byte))?;
        }
    }

    Ok(std::iter::once(multiplicity)
        .chain(std::iter::once(table_sum))
        .chain(channels)
        .collect())
}

/// The single value the row writes, low and high limb, selected off the value bus by
/// each slot's `is_write * active`.
fn written_value<AB: PermutationAirBuilder<F = Felt>>(local: &[AB::Var]) -> (AB::Expr, AB::Expr) {
    (0..BUS_SLOTS).fold(
        (AB::Expr::ZERO, AB::Expr::ZERO),
        |(lo_acc, hi_acc), index| {
            let bus = BusSlot::read(local, index);
            let gate: AB::Expr = bus.is_write * bus.active;
            (lo_acc + gate.dup() * bus.lo, hi_acc + gate * bus.hi)
        },
    )
}

/// The little-endian byte reconstruction `sum byte_i * 256^i` over `count` witness
/// columns starting at `base`.
fn decompose<AB: PermutationAirBuilder<F = Felt>>(
    local: &[AB::Var],
    base: usize,
    count: usize,
) -> AB::Expr {
    (0..count).fold(AB::Expr::ZERO, |acc, byte| {
        acc + local[base + byte] * Felt::new(1u64 << (8 * byte))
    })
}

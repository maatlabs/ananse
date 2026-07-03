//! Program-ROM control-flow lookup: a logderivative (LogUp) accumulator.
//!
//! Every executed row moves the machine along one control-flow edge
//! `(pc, opcode, next_pc, height, imm)`, packed into a single field element. The
//! program ROM is the pinned set of edges the static lift permits. A logarithmic-
//! derivative lookup (Häböck, 2022) proves the multiset of edges the trace takes is a
//! sub-multiset of the ROM: over a verifier challenge `alpha`, the running grand sum
//!
//! ```text
//!   S[i+1] - S[i] = m[i+1] / (alpha - t[i+1]) - 1 / (alpha - f[i+1])
//! ```
//!
//! opens and closes at zero exactly when each edge `f` is a ROM entry `t` and the
//! multiplicities `m` count the lookups. The recurrence is cross-multiplied to stay
//! polynomial. Soundness is Schwartz-Zippel over the challenge, drawn from the
//! quadratic extension so the base-field edges never collide with it.
//!
//! The witness is two permutation-trace columns---the per-entry multiplicities and
//! the grand sum. The looked-up edge `f` is recomputed from the main trace rather
//! than stored, and the table value `t` rides a verifier-filled periodic column, so
//! the ROM never enters the committed trace. Both columns carry a leading zero
//! spacer: ROM entry `j` and its multiplicity sit on row `j + 1`, and the transition
//! into that row absorbs them, which aligns the next-row indexing above with the
//! edge the current row looks up.

use std::collections::HashMap;

use ananse_trace::layout::{COL_HEIGHT, COL_IMM, COL_PC, SELECTOR_BASE};
use ananse_trace::selector::NUM_SELECTORS;
use p3_air::{AirBuilder, ExtensionBuilder, PermutationAirBuilder, WindowAccess};
use p3_field::extension::BinomialExtensionField;
use p3_field::{Dup, Field, PrimeCharacteristicRing, PrimeField64};
use p3_goldilocks::Goldilocks as Felt;
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;

use crate::AirError;
use crate::rom::{HEIGHT_PLACE, IMM_PLACE, NEXT_PC_PLACE, OPCODE_RADIX};

/// The quadratic extension of Goldilocks carrying the LogUp challenge and witness.
pub type Ext = BinomialExtensionField<Felt, 2>;

/// Permutation-trace column holding the per-ROM-entry lookup multiplicities.
pub(crate) const AUX_MULTIPLICITY: usize = 0;
/// Permutation-trace column holding the LogUp grand sum.
pub(crate) const AUX_GRAND_SUM: usize = 1;
/// Number of permutation-trace columns.
pub const AUX_WIDTH: usize = 2;
/// Number of permutation challenges: the single LogUp folding challenge.
pub const NUM_CHALLENGES: usize = 1;

/// The verifier-filled periodic column carrying the program ROM: row `i` holds ROM
/// entry `i`, padded past the table's end with the first entry so the column spans
/// the full trace. `length` is the padded trace height.
pub fn periodic_table(rom: &[Felt], length: usize) -> Vec<Felt> {
    let pad = rom.first().copied().unwrap_or(Felt::ZERO);
    (0..length)
        .map(|row| rom.get(row).copied().unwrap_or(pad))
        .collect()
}

/// Builds the two-column permutation trace---per-ROM-entry multiplicities and the
/// running LogUp grand sum---binding each executed row's control-flow edge to the
/// program `rom`, folded by the challenge `alpha`.
pub fn build_permutation_trace(
    main: &RowMajorMatrix<Felt>,
    rom: &[Felt],
    alpha: Ext,
) -> Result<RowMajorMatrix<Ext>, AirError> {
    let height = main.height();
    let width = main.width();
    let row = |r: usize| &main.values[r * width..(r + 1) * width];

    let table_size = rom.len();
    if table_size > height.saturating_sub(1) {
        return Err(AirError::TraceTooShortForRom {
            trace_len: height,
            rom_len: table_size,
        });
    }

    let index: HashMap<u64, usize> = rom
        .iter()
        .enumerate()
        .map(|(position, entry)| (entry.as_canonical_u64(), position))
        .collect();

    // One lookup per transition row; tally each edge against its ROM position.
    let mut counts = vec![0u64; table_size];
    let edges = (0..height.saturating_sub(1))
        .map(|r| {
            let f = edge(row(r), row(r + 1));
            let position = index.get(&f.as_canonical_u64()).ok_or_else(|| {
                AirError::LookupBuild(format!(
                    "row {r} looks up a control-flow edge absent from the program ROM"
                ))
            })?;
            counts[*position] = counts[*position].saturating_add(1);
            Ok(f)
        })
        .collect::<Result<Vec<Felt>, AirError>>()?;

    let pad = rom.first().copied().unwrap_or(Felt::ZERO);
    let mut multiplicity = vec![Ext::ZERO; height];
    for (position, &count) in counts.iter().enumerate() {
        multiplicity[position + 1] = Ext::from(Felt::new(count));
    }

    // The spacer-shifted table and lookup streams row `i` pairs with: ROM entry `i-1`
    // and the edge the `(i-1)`-th transition looked up.
    let table_at = |i: usize| match i {
        0 => Felt::ZERO,
        i if i - 1 < table_size => rom[i - 1],
        _ => pad,
    };
    let lookup_at = |i: usize| if i == 0 { Felt::ZERO } else { edges[i - 1] };

    let mut grand_sum = vec![Ext::ZERO; height];
    for i in 1..height {
        let t_inv = (alpha - Ext::from(table_at(i)))
            .try_inverse()
            .ok_or(AirError::DegenerateChallenge)?;
        let f_inv = (alpha - Ext::from(lookup_at(i)))
            .try_inverse()
            .ok_or(AirError::DegenerateChallenge)?;
        grand_sum[i] = grand_sum[i - 1] + multiplicity[i] * t_inv - f_inv;
    }

    let values = (0..height)
        .flat_map(|i| [multiplicity[i], grand_sum[i]])
        .collect();
    Ok(RowMajorMatrix::new(values, AUX_WIDTH))
}

/// Evaluates the grand-sum recurrence and its opening/closing boundary, binding the
/// committed multiplicity and grand sum to the edge recomputed from the main trace
/// and the ROM value the periodic column carries.
pub(crate) fn evaluate<AB: PermutationAirBuilder<F = Felt>>(builder: &mut AB) {
    let main = builder.main();
    let perm = builder.permutation();
    let alpha: AB::ExprEF = builder.permutation_randomness()[0].into();
    let table: AB::ExprEF = Into::<AB::Expr>::into(builder.periodic_values()[0]).into();

    let f: AB::ExprEF = edge_expr::<AB>(main.current_slice(), main.next_slice()).into();
    let s_cur = perm.current_slice()[AUX_GRAND_SUM];
    let s_next: AB::ExprEF = perm.next_slice()[AUX_GRAND_SUM].into();
    let m_next: AB::ExprEF = perm.next_slice()[AUX_MULTIPLICITY].into();

    let alpha_minus_f: AB::ExprEF = alpha.dup() - f;
    let alpha_minus_t: AB::ExprEF = alpha - table;
    let s_delta: AB::ExprEF = s_next - Into::<AB::ExprEF>::into(s_cur);
    let lhs: AB::ExprEF = s_delta * alpha_minus_f.dup() * alpha_minus_t.dup();
    let rhs: AB::ExprEF = m_next * alpha_minus_f - alpha_minus_t;
    builder.when_transition().assert_zero_ext(lhs - rhs);

    // The grand sum opens on the first row and closes on the last, both at zero.
    builder.when_first_row().assert_zero_ext(s_cur);
    builder.when_last_row().assert_zero_ext(s_cur);
}

/// The packed schedule edge a row moves along, recomputed in the base field: the
/// one-hot opcode index, the program counter and its successor, the operand-stack
/// height, and the local/global offset, each scaled into its packing place.
fn edge(current: &[Felt], next: &[Felt]) -> Felt {
    let opcode = (0..NUM_SELECTORS).fold(Felt::ZERO, |acc, k| {
        acc + current[SELECTOR_BASE + k] * Felt::new(k as u64)
    });
    opcode
        + current[COL_PC] * Felt::new(OPCODE_RADIX)
        + next[COL_PC] * Felt::new(NEXT_PC_PLACE)
        + current[COL_HEIGHT] * Felt::new(HEIGHT_PLACE)
        + current[COL_IMM] * Felt::new(IMM_PLACE)
}

/// The same packed edge as a constraint expression over the main window, so the
/// lookup side of the recurrence recomputes `f` from the trace rather than trusting a
/// committed column.
fn edge_expr<AB: AirBuilder<F = Felt>>(current: &[AB::Var], next: &[AB::Var]) -> AB::Expr {
    let opcode = (0..NUM_SELECTORS).fold(AB::Expr::ZERO, |acc, k| {
        acc + current[SELECTOR_BASE + k] * Felt::new(k as u64)
    });
    opcode
        + current[COL_PC] * Felt::new(OPCODE_RADIX)
        + next[COL_PC] * Felt::new(NEXT_PC_PLACE)
        + current[COL_HEIGHT] * Felt::new(HEIGHT_PLACE)
        + current[COL_IMM] * Felt::new(IMM_PLACE)
}

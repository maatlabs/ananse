//! Auxiliary segment: the LogUp argument binding the trace's control flow and
//! register layout to the program ROM.

use ananse_trace::layout::{COL_HEIGHT, COL_IMM, COL_PC, SELECTOR_BASE};
use ananse_trace::selector::NUM_SELECTORS;
use maat_air::{LogUpBuiltin, evaluate_transition_step};
use maat_field::{ExtensionOf, Felt, FieldElement};
use winter_air::EvaluationFrame;

use crate::rom::{HEIGHT_PLACE, IMM_PLACE, NEXT_PC_PLACE, OPCODE_RADIX};
use crate::{AirError, error as air_error};

/// Auxiliary column holding the per-ROM-entry lookup multiplicities.
pub(crate) const AUX_MULTIPLICITY: usize = 0;
/// Auxiliary column holding the LogUp grand sum.
pub(crate) const AUX_GRAND_SUM: usize = 1;
/// Number of auxiliary trace columns.
pub const AUX_WIDTH: usize = 2;
/// Number of auxiliary random challenges: the LogUp folding challenge.
pub const NUM_AUX_RANDS: usize = 1;
/// Number of auxiliary transition constraints: the grand-sum recurrence.
pub const NUM_AUX_CONSTRAINTS: usize = 1;
/// Number of auxiliary boundary assertions: the grand sum opens and closes at zero.
pub(crate) const NUM_AUX_ASSERTIONS: usize = 2;

/// The single LogUp table identifier: the program ROM.
const PROGRAM_ROM_TABLE: u32 = 0;

/// Builds the two auxiliary columns---per-entry multiplicities and the running grand
/// sum---for the program-ROM LogUp over the main `columns`, folded by the verifier
/// challenge `alpha`. One edge is looked up per row (the last row has no successor
/// and contributes none), matching the one-edge-per-row table layout.
pub fn build_aux_columns(
    columns: &[Vec<Felt>],
    rom: &[Felt],
    length: usize,
    alpha: Felt,
) -> Result<[Vec<Felt>; 2], AirError> {
    let mut logup = LogUpBuiltin::new();
    logup
        .register_table(PROGRAM_ROM_TABLE, rom.to_vec())
        .map_err(air_error::build_error)?;
    for row in 0..length.saturating_sub(1) {
        let current = row_slice(columns, row);
        let next = row_slice(columns, row + 1);
        logup
            .register_lookup(PROGRAM_ROM_TABLE, edge(&current, &next))
            .map_err(air_error::build_error)?;
    }
    let built = logup
        .build_columns::<Felt>(length, alpha)
        .map_err(air_error::build_error)?
        .into_iter()
        .next()
        .ok_or_else(|| AirError::LookupBuild("program ROM produced no witness".into()))?;
    Ok([built.multiplicities, built.grand_sum])
}

/// The periodic table column the recurrence reads: ROM entry `i` on row `i`, then
/// the first entry repeated. Row `i` carries the table value the grand-sum step at
/// row `i` pairs with the edge that row looks up. The verifier fills it from the
/// public program alone.
pub fn periodic_table(rom: &[Felt], length: usize) -> Vec<Felt> {
    let pad = rom.first().copied().unwrap_or(Felt::ZERO);
    (0..length)
        .map(|row| rom.get(row).copied().unwrap_or(pad))
        .collect()
}

/// Evaluates the LogUp grand-sum recurrence on one `current`/`next` row pair, writing
/// its single residual into `result`. `table_next` is the periodic table value on
/// the current row; `alpha` is the folding challenge. The residual vanishes when the
/// grand sum advances by `m / (alpha - t) - 1 / (alpha - f)`, binding the committed
/// multiplicities and grand sum to the edge recomputed from the main trace and the
/// ROM value from the periodic column.
pub(crate) fn evaluate_transition<F, E>(
    main_frame: &EvaluationFrame<F>,
    aux_frame: &EvaluationFrame<E>,
    table_next: F,
    alpha: E,
    result: &mut [E],
) where
    F: FieldElement<BaseField = Felt>,
    E: FieldElement<BaseField = Felt> + ExtensionOf<F>,
{
    let f_next = edge(main_frame.current(), main_frame.next());
    result[0] = evaluate_transition_step(
        aux_frame.current()[AUX_GRAND_SUM],
        aux_frame.next()[AUX_GRAND_SUM],
        aux_frame.next()[AUX_MULTIPLICITY],
        f_next,
        table_next,
        alpha,
    );
}

/// The packed schedule edge a row moves along, as a field expression over the main
/// trace: `opcode + pc * OPCODE_RADIX + next_pc * NEXT_PC_PLACE + height *
/// HEIGHT_PLACE + imm * IMM_PLACE`, with the opcode index read out of the one-hot
/// selectors and the height and offset read out of their columns. Evaluating it in
/// the field (rather than on integers) keeps it usable in the transition constraint,
/// where the trace columns are field elements of a possibly-extension field.
fn edge<F>(current: &[F], next: &[F]) -> F
where
    F: FieldElement<BaseField = Felt>,
{
    let opcode = (0..NUM_SELECTORS).fold(F::ZERO, |acc, index| {
        acc + current[SELECTOR_BASE + index] * F::from(Felt::new(index as u64))
    });
    opcode
        + current[COL_PC] * F::from(Felt::new(OPCODE_RADIX))
        + next[COL_PC] * F::from(Felt::new(NEXT_PC_PLACE))
        + current[COL_HEIGHT] * F::from(Felt::new(HEIGHT_PLACE))
        + current[COL_IMM] * F::from(Felt::new(IMM_PLACE))
}

/// Extracts the values of every column on `row` from the column-major matrix.
fn row_slice(columns: &[Vec<Felt>], row: usize) -> Vec<Felt> {
    columns.iter().map(|column| column[row]).collect()
}

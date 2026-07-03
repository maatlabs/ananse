//! Register-shaped AIR for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped algebraic
//! intermediate representation. This crate defines [`AnanseAir`], the single
//! [`Air`] the STARK prover and verifier evaluate against the trace
//! [`ananse_trace`] produces.

#![forbid(unsafe_code)]

mod bus;
mod constraints;
mod error;
mod rom;

use ananse_trace::layout::{
    BUS_SLOTS, COL_CLK, COL_HEIGHT, COL_PC, RANGE_COLS, SELECTOR_BASE, main_width,
};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
use constraints::{address, boundary, consistency, logup, numeric, permutation, range};
pub use error::AirError;
use p3_air::{Air, AirBuilder, BaseAir, PermutationAirBuilder, WindowAccess};
use p3_field::PrimeCharacteristicRing;
use p3_field::extension::BinomialExtensionField;
use p3_goldilocks::Goldilocks as Felt;
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;
pub use rom::{pack_edge, program_rom};

use crate::bus::{BusSlot, SortedEntry};

/// Result alias for AIR operations.
pub type Result<T> = core::result::Result<T, AirError>;

/// The quadratic extension of Goldilocks carrying the permutation-argument
/// challenges and the LogUp and multiset-equality witnesses.
pub type Ext = BinomialExtensionField<Felt, 2>;

/// Permutation-trace column holding the per-ROM-entry control-flow multiplicities.
pub(crate) const AUX_MULTIPLICITY: usize = 0;
/// Permutation-trace column holding the control-flow LogUp grand sum.
pub(crate) const AUX_GRAND_SUM: usize = 1;
/// Permutation-trace column holding the execution-order value-bus accumulator.
pub(crate) const AUX_BUS_ACC: usize = 2;
/// Permutation-trace column holding the address-sorted-log accumulator.
pub(crate) const AUX_SORTED_ACC: usize = 3;
/// Permutation-trace column holding the byte-table per-value multiplicity.
pub(crate) const AUX_RC_MULT: usize = 4;
/// Permutation-trace column holding the byte-table's table-side running reciprocal.
pub(crate) const AUX_RC_TABLE: usize = 5;
/// First permutation-trace column of the byte-table channel reciprocals, one per
/// range-check witness column.
pub(crate) const AUX_RC_CHANNEL_BASE: usize = 6;
/// Permutation-trace column holding the boundary lookup's per-table-entry multiplicity.
pub(crate) const AUX_BND_MULT: usize = AUX_RC_CHANNEL_BASE + RANGE_COLS;
/// Permutation-trace column holding the boundary lookup's table-side running reciprocal.
pub(crate) const AUX_BND_TABLE: usize = AUX_BND_MULT + 1;
/// First permutation-trace column of the boundary lookup's channel reciprocals, one
/// per sorted-log entry.
pub(crate) const AUX_BND_CHANNEL_BASE: usize = AUX_BND_MULT + 2;
/// Number of permutation-trace columns.
pub const AUX_WIDTH: usize = AUX_BND_CHANNEL_BASE + BUS_SLOTS;

/// Challenge index of the control-flow lookup's folding challenge.
pub(crate) const CHALLENGE_CONTROL_FLOW: usize = 0;
/// Challenge index of the consistency permutation's logderivative denominator.
pub(crate) const CHALLENGE_DENOM: usize = 1;
/// Challenge index of the access-folding challenge.
pub(crate) const CHALLENGE_FOLD: usize = 2;
/// Challenge index of the range-check byte-table's logderivative denominator.
pub(crate) const CHALLENGE_RANGE: usize = 3;
/// Challenge index of the boundary lookup's logderivative denominator.
pub(crate) const CHALLENGE_BOUNDARY: usize = 4;
/// Number of permutation challenges.
pub const NUM_CHALLENGES: usize = 5;

pub fn build_permutation_trace(
    main: &RowMajorMatrix<Felt>,
    rom: &[Felt],
    initial: &[(u64, Felt, Felt)],
    challenges: [Ext; NUM_CHALLENGES],
) -> Result<RowMajorMatrix<Ext>> {
    let (multiplicity, grand_sum) =
        logup::control_flow_columns(main, rom, challenges[CHALLENGE_CONTROL_FLOW])?;
    let (bus_acc, sorted_acc) = permutation::consistency_columns(
        main,
        challenges[CHALLENGE_DENOM],
        challenges[CHALLENGE_FOLD],
    )?;
    let range = range::columns(main, challenges[CHALLENGE_RANGE])?;
    let boundary = boundary::columns(
        main,
        initial,
        challenges[CHALLENGE_FOLD],
        challenges[CHALLENGE_BOUNDARY],
    )?;

    let aux = [multiplicity, grand_sum, bus_acc, sorted_acc]
        .into_iter()
        .chain(range)
        .chain(boundary)
        .collect::<Vec<Vec<Ext>>>();
    debug_assert_eq!(aux.len(), AUX_WIDTH);
    let values = (0..main.height())
        .flat_map(|i| aux.iter().map(move |column| column[i]))
        .collect();
    Ok(RowMajorMatrix::new(values, AUX_WIDTH))
}

pub struct AnanseAir {
    rom_periodic: Vec<Felt>,
    byte_table: Vec<Felt>,
    boundary: [Vec<Felt>; 3],
    stack_base: u64,
}

impl AnanseAir {
    pub fn new(
        rom: &[Felt],
        trace_len: usize,
        stack_base: u32,
        initial: &[(u64, Felt, Felt)],
    ) -> Self {
        Self {
            rom_periodic: logup::periodic_table(rom, trace_len),
            byte_table: range::byte_table(trace_len),
            boundary: boundary::periodic_columns(initial, trace_len),
            stack_base: u64::from(stack_base),
        }
    }
}

impl BaseAir<Felt> for AnanseAir {
    fn width(&self) -> usize {
        main_width()
    }

    fn num_periodic_columns(&self) -> usize {
        5
    }

    fn periodic_columns(&self) -> Vec<Vec<Felt>> {
        vec![
            self.rom_periodic.clone(),
            self.byte_table.clone(),
            self.boundary[0].clone(),
            self.boundary[1].clone(),
            self.boundary[2].clone(),
        ]
    }

    fn periodic_values(&self, row_index: usize) -> Vec<Felt> {
        vec![
            self.rom_periodic[row_index % self.rom_periodic.len()],
            self.byte_table[row_index % self.byte_table.len()],
            self.boundary[0][row_index % self.boundary[0].len()],
            self.boundary[1][row_index % self.boundary[1].len()],
            self.boundary[2][row_index % self.boundary[2].len()],
        ]
    }
}

impl<AB: PermutationAirBuilder<F = Felt>> Air<AB> for AnanseAir {
    fn eval(&self, builder: &mut AB) {
        let main = builder.main();
        let local = main.current_slice();
        let next = main.next_slice();

        init_eval(builder, local, next);
        numeric::evaluate(builder, local);
        consistency::evaluate(builder, local, next);
        address::evaluate(builder, local, self.stack_base);
        logup::evaluate(builder);
        permutation::evaluate(builder);
        range::evaluate(builder);
        boundary::evaluate(builder);
    }
}

fn init_eval<AB: AirBuilder<F = Felt>>(builder: &mut AB, local: &[AB::Var], next: &[AB::Var]) {
    let selectors = &local[SELECTOR_BASE..SELECTOR_BASE + NUM_SELECTORS];
    for &selector in selectors {
        builder.assert_bool(selector);
    }
    let one_hot_sum = selectors
        .iter()
        .copied()
        .map(Into::into)
        .fold(AB::Expr::ZERO, |acc, s: AB::Expr| acc + s);
    builder.assert_one(one_hot_sum);

    // Every value-bus and sorted-log flag is boolean.
    for index in 0..BUS_SLOTS {
        let bus = BusSlot::read(local, index);
        builder.assert_bool(bus.active);
        builder.assert_bool(bus.is_write);
        let sorted = SortedEntry::read(local, index);
        builder.assert_bool(sorted.active);
        builder.assert_bool(sorted.is_write);
        builder.assert_bool(sorted.same_addr);
    }

    let pad = local[SELECTOR_BASE + SEL_PADDING];
    for index in 0..BUS_SLOTS {
        builder.assert_zero(pad * BusSlot::read(local, index).active);
        builder.assert_zero(pad * SortedEntry::read(local, index).active);
    }

    let pad_next = next[SELECTOR_BASE + SEL_PADDING];
    builder
        .when_transition()
        .assert_zero(pad * (AB::Expr::ONE - pad_next));

    builder
        .when_transition()
        .assert_zero(next[COL_CLK] - local[COL_CLK] - AB::Expr::ONE);

    builder.when_first_row().assert_zero(local[COL_PC]);
    builder.when_first_row().assert_zero(local[COL_CLK]);
    builder.when_first_row().assert_zero(local[COL_HEIGHT]);
    builder
        .when_last_row()
        .assert_one(local[SELECTOR_BASE + SEL_PADDING]);
}

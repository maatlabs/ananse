use ananse_trace::layout::{
    BUS_SLOTS, BW_NIBBLES, COL_CLK, COL_HEIGHT, COL_PC, POPCNT_BYTES, RANGE_COLS, SELECTOR_BASE,
    main_width,
};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
use p3_air::{Air, AirBuilder, BaseAir, PermutationAirBuilder, WindowAccess};
use p3_field::PrimeCharacteristicRing;

use crate::Felt;
use crate::bus::{BusSlot, SortedEntry};

pub mod address;
pub mod bitwise;
pub mod boundary;
pub mod comparison;
pub mod consistency;
pub mod control;
pub mod logup;
pub mod movement;
pub mod numeric;
pub mod permutation;
pub mod popcount;
pub mod range;

/// Number of permutation challenges.
pub const NUM_CHALLENGES: usize = 11;

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
/// Permutation-trace column holding the nibble AND-table's per-entry multiplicity.
pub(crate) const AUX_BW_MULT: usize = AUX_BND_CHANNEL_BASE + BUS_SLOTS;
/// Permutation-trace column holding the AND-table's table-side running reciprocal.
pub(crate) const AUX_BW_TABLE: usize = AUX_BW_MULT + 1;
/// First permutation-trace column of the AND-table channel reciprocals, one per
/// operand nibble position.
pub(crate) const AUX_BW_CHANNEL_BASE: usize = AUX_BW_MULT + 2;
/// Permutation-trace column holding the popcount table's per-entry multiplicity.
pub(crate) const AUX_PC_MULT: usize = AUX_BW_CHANNEL_BASE + BW_NIBBLES;
/// Permutation-trace column holding the popcount table's table-side running reciprocal.
pub(crate) const AUX_PC_TABLE: usize = AUX_PC_MULT + 1;
/// First permutation-trace column of the popcount channel reciprocals, one per
/// operand byte position.
pub(crate) const AUX_PC_CHANNEL_BASE: usize = AUX_PC_MULT + 2;
/// Permutation-trace column holding the data-ROM lookup's per-entry multiplicity.
pub(crate) const AUX_DATA_MULT: usize = AUX_PC_CHANNEL_BASE + POPCNT_BYTES;
/// Permutation-trace column holding the data-ROM's table-side running reciprocal.
pub(crate) const AUX_DATA_TABLE: usize = AUX_DATA_MULT + 1;
/// Permutation-trace column holding the data-ROM's single condition-gated lookup
/// channel: it advances only on a `const` or conditional-branch row.
pub(crate) const AUX_DATA_CHANNEL: usize = AUX_DATA_MULT + 2;
/// Number of permutation-trace columns.
pub const AUX_WIDTH: usize = AUX_DATA_CHANNEL + 1;

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
/// Challenge index folding a bitwise nibble triple `(a, b, a & b)` into one element.
pub(crate) const CHALLENGE_BITWISE_FOLD: usize = 5;
/// Challenge index of the nibble AND-table's logderivative denominator.
pub(crate) const CHALLENGE_BITWISE_DENOM: usize = 6;
/// Challenge index folding a popcount pair `(byte, popcount(byte))` into one element.
pub(crate) const CHALLENGE_POPCOUNT_FOLD: usize = 7;
/// Challenge index of the popcount table's logderivative denominator.
pub(crate) const CHALLENGE_POPCOUNT_DENOM: usize = 8;
/// Challenge index folding a data-ROM tuple `(pc, a, b)` into one element.
pub(crate) const CHALLENGE_DATA_FOLD: usize = 9;
/// Challenge index of the data-ROM lookup's logderivative denominator.
pub(crate) const CHALLENGE_DATA_DENOM: usize = 10;

pub struct AnanseAir {
    rom_periodic: Vec<Felt>,
    byte_table: Vec<Felt>,
    boundary: [Vec<Felt>; 3],
    and_table: [Vec<Felt>; 3],
    popcount_table: Vec<Felt>,
    data_table: [Vec<Felt>; 3],
    stack_base: u64,
}

impl AnanseAir {
    pub fn new(
        rom: &[Felt],
        data: &[(u32, u32, u32)],
        trace_len: usize,
        stack_base: u32,
        initial: &[(u64, Felt, Felt)],
    ) -> Self {
        Self {
            rom_periodic: logup::periodic_table(rom, trace_len),
            byte_table: range::byte_table(trace_len),
            boundary: boundary::periodic_columns(initial, trace_len),
            and_table: bitwise::and_table(trace_len),
            popcount_table: popcount::table(trace_len),
            data_table: control::data_periodic(data, trace_len),
            stack_base: u64::from(stack_base),
        }
    }
}

impl BaseAir<Felt> for AnanseAir {
    fn width(&self) -> usize {
        main_width()
    }

    fn num_periodic_columns(&self) -> usize {
        12
    }

    fn periodic_columns(&self) -> Vec<Vec<Felt>> {
        vec![
            self.rom_periodic.clone(),
            self.byte_table.clone(),
            self.boundary[0].clone(),
            self.boundary[1].clone(),
            self.boundary[2].clone(),
            self.and_table[0].clone(),
            self.and_table[1].clone(),
            self.and_table[2].clone(),
            self.popcount_table.clone(),
            self.data_table[0].clone(),
            self.data_table[1].clone(),
            self.data_table[2].clone(),
        ]
    }

    fn periodic_values(&self, row_index: usize) -> Vec<Felt> {
        vec![
            self.rom_periodic[row_index % self.rom_periodic.len()],
            self.byte_table[row_index % self.byte_table.len()],
            self.boundary[0][row_index % self.boundary[0].len()],
            self.boundary[1][row_index % self.boundary[1].len()],
            self.boundary[2][row_index % self.boundary[2].len()],
            self.and_table[0][row_index % self.and_table[0].len()],
            self.and_table[1][row_index % self.and_table[1].len()],
            self.and_table[2][row_index % self.and_table[2].len()],
            self.popcount_table[row_index % self.popcount_table.len()],
            self.data_table[0][row_index % self.data_table[0].len()],
            self.data_table[1][row_index % self.data_table[1].len()],
            self.data_table[2][row_index % self.data_table[2].len()],
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
        movement::evaluate(builder, local);
        comparison::evaluate(builder, local);
        control::evaluate(builder);
        consistency::evaluate(builder, local, next);
        address::evaluate(builder, local, self.stack_base);
        logup::evaluate(builder);
        permutation::evaluate(builder);
        range::evaluate(builder);
        boundary::evaluate(builder);
        bitwise::evaluate(builder);
        popcount::evaluate(builder);
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

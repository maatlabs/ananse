//! Register-shaped AIR for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped algebraic
//! intermediate representation. This crate defines [`AnanseAir`], the single
//! [`Air`] the STARK prover and verifier evaluate against the trace
//! [`ananse_trace`] produces, together with the program-ROM table and the LogUp
//! permutation trace that binds the trace's control flow and register layout to it.

#![forbid(unsafe_code)]

mod address;
mod bus;
mod consistency;
mod error;
mod logup;
mod numeric;
mod rom;

use ananse_trace::layout::{BUS_SLOTS, COL_CLK, COL_HEIGHT, COL_PC, SELECTOR_BASE, main_width};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
pub use error::AirError;
pub use logup::{AUX_WIDTH, Ext, NUM_CHALLENGES, build_permutation_trace, periodic_table};
use p3_air::{Air, AirBuilder, BaseAir, PermutationAirBuilder, WindowAccess};
use p3_field::PrimeCharacteristicRing;
use p3_goldilocks::Goldilocks as Felt;
pub use rom::{pack_edge, program_rom};

use crate::bus::{BusSlot, SortedEntry};

/// The register-shaped AIR: one main trace carrying the unified access-log, plus a
/// permutation trace (built by [`build_permutation_trace`]) carrying the LogUp
/// witness that binds the trace's control flow and layout to the program ROM.
pub struct AnanseAir {
    rom_periodic: Vec<Felt>,
    stack_base: u64,
}

impl AnanseAir {
    /// Builds the AIR for a program ROM and the frame's operand-stack base. `rom` is
    /// the packed ROM table from [`program_rom`]; `trace_len` is the padded trace
    /// height the verifier-filled periodic ROM column spans.
    pub fn new(rom: &[Felt], trace_len: usize, stack_base: u32) -> Self {
        Self {
            rom_periodic: periodic_table(rom, trace_len),
            stack_base: u64::from(stack_base),
        }
    }
}

impl BaseAir<Felt> for AnanseAir {
    fn width(&self) -> usize {
        main_width()
    }

    fn num_periodic_columns(&self) -> usize {
        1
    }

    fn periodic_columns(&self) -> Vec<Vec<Felt>> {
        vec![self.rom_periodic.clone()]
    }

    fn periodic_values(&self, row_index: usize) -> Vec<Felt> {
        vec![self.rom_periodic[row_index % self.rom_periodic.len()]]
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
    }
}

/// Geometry-independent constraints: one-hot opcode selectors, value-bus and
/// sorted-log flag booleanity, the padding absorber, the clock increment, and the
/// entry/exit boundary.
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

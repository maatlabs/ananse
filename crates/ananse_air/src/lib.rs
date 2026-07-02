//! Register-shaped AIR for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped algebraic
//! intermediate representation. This crate defines [`AnanseAir`], the single
//! Winterfell [`Air`] the STARK prover and verifier evaluate against the
//! column-major trace [`ananse_trace`] produces.
//!
//! One WebAssembly operator is one trace block. The operand stack, locals, globals,
//! and linear memory are lifted into one flat address space, and every access rides
//! a fixed-width value bus. Consistency across the whole machine is one argument: the
//! address-sorted access log is a permutation of the execution-order bus and returns,
//! on each read, the value the previous access to that cell left. The main segment
//! carries the bus, the sorted log, and the per-opcode value semantics; the auxiliary
//! segment carries the permutation and the control-flow-and-layout lookup against the
//! program ROM.

#![forbid(unsafe_code)]

mod address;
mod aux_segment;
mod bus;
mod consistency;
mod error;
mod numeric;
mod rom;
mod transition;

use ananse_trace::layout::{COL_CLK, COL_HEIGHT, COL_PC, SELECTOR_BASE};
use ananse_trace::selector::SEL_PADDING;
pub use aux_segment::{
    AUX_WIDTH, NUM_AUX_CONSTRAINTS, NUM_AUX_RANDS, build_aux_columns, periodic_table,
};
pub use error::AirError;
use maat_field::{ExtensionOf, Felt, FieldElement, ToElements};
pub use rom::{pack_edge, program_rom};
pub use winter_air::{
    Air, AuxRandElements, BatchingMethod, EvaluationFrame, FieldExtension, ProofOptions, TraceInfo,
};
use winter_air::{AirContext, Assertion, TransitionConstraintDegree};

/// Number of main-segment boundary assertions: the entry program counter, the entry
/// clock, the entry operand-stack height, and the padding-pinned final row.
const NUM_ASSERTIONS: usize = 4;

/// The single auxiliary transition constraint is the LogUp grand-sum recurrence,
/// declared at degree three conservatively (the periodic table value keeps the
/// realized degree lower).
fn aux_degrees() -> Vec<TransitionConstraintDegree> {
    vec![TransitionConstraintDegree::new(3)]
}

/// The register-shaped AIR: a two-segment Winterfell [`Air`]. The main segment
/// carries the unified access-log trace; the auxiliary segment carries the LogUp
/// witness binding the trace's control flow and layout to the program ROM.
pub struct AnanseAir {
    context: AirContext<Felt>,
    program: Vec<Felt>,
    stack_base: u64,
}

/// Public inputs bound into the proof transcript.
#[derive(Debug, Clone, Default)]
pub struct AnansePublicInputs {
    /// The packed program ROM (see [`program_rom()`]).
    pub program: Vec<Felt>,
    /// Register-file offset at which the operand stack begins (locals plus globals).
    pub stack_base: u32,
}

impl AnansePublicInputs {
    pub fn new(program: Vec<Felt>, stack_base: u32) -> Self {
        Self {
            program,
            stack_base,
        }
    }
}

impl ToElements<Felt> for AnansePublicInputs {
    fn to_elements(&self) -> Vec<Felt> {
        self.program
            .iter()
            .copied()
            .chain([Felt::new(u64::from(self.stack_base))])
            .collect()
    }
}

impl AnanseAir {
    pub fn num_main_transition_constraints(&self) -> usize {
        transition::NUM_CONSTRAINTS
    }
}

impl Air for AnanseAir {
    type BaseField = Felt;
    type PublicInputs = AnansePublicInputs;

    fn new(trace_info: TraceInfo, pub_inputs: Self::PublicInputs, options: ProofOptions) -> Self {
        let context = AirContext::new_multi_segment(
            trace_info,
            transition::degrees(),
            aux_degrees(),
            NUM_ASSERTIONS,
            aux_segment::NUM_AUX_ASSERTIONS,
            options,
        );
        Self {
            context,
            program: pub_inputs.program,
            stack_base: u64::from(pub_inputs.stack_base),
        }
    }

    fn context(&self) -> &AirContext<Self::BaseField> {
        &self.context
    }

    fn evaluate_transition<E: FieldElement<BaseField = Self::BaseField>>(
        &self,
        frame: &EvaluationFrame<E>,
        _periodic_values: &[E],
        result: &mut [E],
    ) {
        transition::evaluate(frame.current(), frame.next(), self.stack_base, result);
    }

    fn evaluate_aux_transition<F, E>(
        &self,
        main_frame: &EvaluationFrame<F>,
        aux_frame: &EvaluationFrame<E>,
        periodic_values: &[F],
        aux_rand_elements: &AuxRandElements<E>,
        result: &mut [E],
    ) where
        F: FieldElement<BaseField = Self::BaseField>,
        E: FieldElement<BaseField = Self::BaseField> + ExtensionOf<F>,
    {
        aux_segment::evaluate_transition(
            main_frame,
            aux_frame,
            periodic_values[0],
            aux_rand_elements.rand_elements()[0],
            result,
        );
    }

    fn get_periodic_column_values(&self) -> Vec<Vec<Self::BaseField>> {
        vec![periodic_table(
            &self.program,
            self.context.trace_info().length(),
        )]
    }

    fn get_assertions(&self) -> Vec<Assertion<Self::BaseField>> {
        let last_row = self.context.trace_info().length().saturating_sub(1);
        vec![
            Assertion::single(COL_PC, 0, Felt::ZERO),
            Assertion::single(COL_CLK, 0, Felt::ZERO),
            Assertion::single(COL_HEIGHT, 0, Felt::ZERO),
            Assertion::single(SELECTOR_BASE + SEL_PADDING, last_row, Felt::ONE),
        ]
    }

    fn get_aux_assertions<E: FieldElement<BaseField = Self::BaseField>>(
        &self,
        _aux_rand_elements: &AuxRandElements<E>,
    ) -> Vec<Assertion<E>> {
        let last_row = self.context.trace_info().length().saturating_sub(1);
        vec![
            Assertion::single(aux_segment::AUX_GRAND_SUM, 0, E::ZERO),
            Assertion::single(aux_segment::AUX_GRAND_SUM, last_row, E::ZERO),
        ]
    }
}

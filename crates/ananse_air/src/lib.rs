//! Register-shaped AIR for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped algebraic
//! intermediate representation. This crate defines [`AnanseAir`], the single
//! Winterfell [`Air`] the STARK prover and verifier evaluate against the
//! column-major trace [`ananse_trace`] produces. One WebAssembly operator is one
//! trace block, and the transition function is a sum over opcodes: each opcode's
//! per-block constraint is gated by its one-hot selector so it fires only on that
//! opcode's row. The register bank (operand stack, locals, and globals) is a set
//! of static columns, so only linear memory keeps a permutation argument.

#![forbid(unsafe_code)]

mod transition;

use ananse_trace::layout::COL_PC;
use maat_field::{Felt, FieldElement, ToElements};
pub use transition::NUM_TRANSITION_CONSTRAINTS;
pub use winter_air::{
    Air, BatchingMethod, EvaluationFrame, FieldExtension, ProofOptions, TraceInfo,
};
use winter_air::{AirContext, Assertion};

/// Number of boundary assertions on the trace.
const NUM_ASSERTIONS: usize = 1;

pub struct AnanseAir {
    context: AirContext<Felt>,
}

/// Public inputs bound into the proof transcript.
#[derive(Debug, Clone, Default)]
pub struct AnansePublicInputs;

impl ToElements<Felt> for AnansePublicInputs {
    fn to_elements(&self) -> Vec<Felt> {
        Vec::new()
    }
}

impl Air for AnanseAir {
    type BaseField = Felt;
    type PublicInputs = AnansePublicInputs;

    fn new(trace_info: TraceInfo, _pub_inputs: Self::PublicInputs, options: ProofOptions) -> Self {
        let context = AirContext::new(trace_info, transition::degrees(), NUM_ASSERTIONS, options);
        Self { context }
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
        transition::evaluate(frame.current(), result);
    }

    fn get_assertions(&self) -> Vec<Assertion<Self::BaseField>> {
        // The first executed operator sits at program-counter index zero.
        vec![Assertion::single(COL_PC, 0, Felt::ZERO)]
    }
}

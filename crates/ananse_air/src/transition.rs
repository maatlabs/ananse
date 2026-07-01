//! Transition constraints over the register-shaped trace.
//!
//! A transition constraint is a polynomial identity that must vanish on every
//! row of the trace. Each opcode's own semantics are enforced by multiplying its
//! constraint by the one-hot selector for that opcode, so those constraints fire
//! only on the opcode's own block.

use ananse_trace::layout::SELECTOR_BASE;
use ananse_trace::selector::NUM_SELECTORS;
use maat_field::FieldElement;
use winter_air::TransitionConstraintDegree;

/// Number of transition constraints: one booleanity constraint per selector
/// column plus a single one-hot sum constraint.
pub const NUM_TRANSITION_CONSTRAINTS: usize = NUM_SELECTORS + 1;

/// Algebraic degree of each transition constraint, in the order
/// [`evaluate`] writes them: the per-selector booleanity constraints
/// `s * (s - 1)` are degree two, and the one-hot sum `(Σ s) - 1` is degree one.
pub(crate) fn degrees() -> Vec<TransitionConstraintDegree> {
    (0..NUM_SELECTORS)
        .map(|_| TransitionConstraintDegree::new(2))
        .chain(core::iter::once(TransitionConstraintDegree::new(1)))
        .collect()
}

/// Evaluates the transition constraints on one trace row, writing one residual
/// per constraint into `result`. Every residual is zero exactly when the row's
/// opcode selectors are one-hot: each selector is a bit (`s * (s - 1) = 0`) and
/// their sum is one (`(Σ s) - 1 = 0`).
pub(crate) fn evaluate<E: FieldElement>(current: &[E], result: &mut [E]) {
    let selectors = &current[SELECTOR_BASE..SELECTOR_BASE + NUM_SELECTORS];
    for (residual, &selector) in result.iter_mut().zip(selectors) {
        *residual = selector * (selector - E::ONE);
    }
    let one_hot = selectors.iter().copied().fold(E::ZERO, |acc, s| acc + s);
    result[NUM_SELECTORS] = one_hot - E::ONE;
}

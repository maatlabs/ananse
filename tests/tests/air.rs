use ananse_air::{Air, NUM_TRANSITION_CONSTRAINTS};
use ananse_tests::{
    SINGLE_FRAME_FIXTURES, air_for, trace_of, transition_violations, wat_from_file,
};
use ananse_trace::layout::{COL_PC, SELECTOR_BASE};
use ananse_trace::selector::SEL_PADDING;
use maat_field::{Felt, FieldElement};

#[test]
fn every_single_frame_trace_satisfies_the_transition_system() {
    for name in SINGLE_FRAME_FIXTURES {
        let trace = trace_of(&wat_from_file(name));
        let air = air_for(&trace);
        let violations = transition_violations(&air, trace.columns(), trace.length());
        assert!(violations.is_empty(), "{name}: {violations:?}");
        // The property the boundary assertion pins: the entry begins at PC zero.
        assert_eq!(trace.columns()[COL_PC][0], Felt::ZERO, "{name}: entry PC");
    }
}

#[test]
fn air_dimensions_and_assertions_match_the_trace() {
    let trace = trace_of(&wat_from_file("func_add.wat"));
    let air = air_for(&trace);
    assert_eq!(air.context().trace_info().main_trace_width(), trace.width());
    assert_eq!(
        air.context().num_transition_constraints(),
        NUM_TRANSITION_CONSTRAINTS
    );
    let assertions = air.get_assertions();
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].column(), COL_PC);
}

#[test]
fn tampering_a_selector_breaks_the_transition_system() {
    let trace = trace_of(&wat_from_file("func_add.wat"));
    let air = air_for(&trace);

    // A pristine trace is clean; forcing a second hot selector on row 0---the
    // padding selector, otherwise zero on a real step---makes the row's
    // selectors sum to two and breaks one-hotness.
    assert!(transition_violations(&air, trace.columns(), trace.length()).is_empty());
    let mut columns = trace.columns().to_vec();
    columns[SELECTOR_BASE + SEL_PADDING][0] = Felt::ONE;
    let violations = transition_violations(&air, &columns, trace.length());
    assert!(
        violations.iter().any(|&(row, _)| row == 0),
        "expected a row-0 violation, got {violations:?}"
    );
}

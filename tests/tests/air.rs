use ananse_air::{Air, NUM_AUX_CONSTRAINTS, build_aux_columns, periodic_table};
use ananse_tests::{
    SINGLE_FRAME_FIXTURES, air_for, aux_residuals, aux_violations, trace_and_rom,
    transition_violations, wat_from_file,
};
use ananse_trace::layout::{COL_CLK, COL_HEIGHT, COL_PC, SELECTOR_BASE};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
use maat_field::{Felt, FieldElement};

/// A fixed stand-in for the Fiat--Shamir folding challenge the prover draws, letting
/// the auxiliary lookup be exercised without the prover.
fn mock_challenge() -> Felt {
    Felt::new(0x9e37_79b9_7f4a_7c15)
}

#[test]
fn every_single_frame_trace_satisfies_the_transition_system() {
    for name in SINGLE_FRAME_FIXTURES {
        let (trace, rom) = trace_and_rom(&wat_from_file(name));
        let air = air_for(&trace, rom);
        let violations = transition_violations(&air, trace.columns(), trace.length());
        assert!(violations.is_empty(), "{name}: {violations:?}");
        // The property the boundary assertion pins: the entry begins at PC zero.
        assert_eq!(trace.columns()[COL_PC][0], Felt::ZERO, "{name}: entry PC");
    }
}

#[test]
fn air_dimensions_and_assertions_match_the_trace() {
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, rom);
    assert_eq!(air.context().trace_info().main_trace_width(), trace.width());
    assert_eq!(
        air.context().num_transition_constraints(),
        air.num_main_transition_constraints() + NUM_AUX_CONSTRAINTS
    );
    let assertions = air.get_assertions();
    assert_eq!(assertions.len(), 4);
    assert_eq!(assertions[0].column(), COL_PC);
    assert_eq!(assertions[1].column(), COL_CLK);
    assert_eq!(assertions[2].column(), COL_HEIGHT);
    assert_eq!(assertions[3].column(), SELECTOR_BASE + SEL_PADDING);
}

#[test]
fn tampering_a_selector_breaks_the_transition_system() {
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, rom);

    // A pristine trace is clean; forcing a second hot selector on row 0---the padding
    // selector, otherwise zero on a real step---makes the row's selectors sum to two
    // and breaks one-hotness.
    assert!(transition_violations(&air, trace.columns(), trace.length()).is_empty());
    let mut columns = trace.columns().to_vec();
    columns[SELECTOR_BASE + SEL_PADDING][0] = Felt::ONE;
    let violations = transition_violations(&air, &columns, trace.length());
    assert!(
        violations.iter().any(|&(row, _)| row == 0),
        "expected a row-0 violation, got {violations:?}"
    );
}

#[test]
fn breaking_the_clock_breaks_the_transition_system() {
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, rom);

    // The clock advances by one every row; stalling it on row 1 makes row 0's
    // increment constraint fail, so timestamps cannot be reused across accesses.
    assert!(transition_violations(&air, trace.columns(), trace.length()).is_empty());
    let mut columns = trace.columns().to_vec();
    columns[COL_CLK][1] = columns[COL_CLK][0];
    let violations = transition_violations(&air, &columns, trace.length());
    assert!(
        violations.iter().any(|&(row, _)| row == 0),
        "expected a row-0 clock violation, got {violations:?}"
    );
}

#[test]
fn clearing_padding_inside_the_halt_suffix_breaks_the_transition_system() {
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, rom);

    // Padding is absorbing: clearing the padding selector on a row inside the halt
    // suffix makes the preceding still-padding row's absorbing constraint fail, so a
    // prover cannot revive a real step in the trace tail.
    assert!(transition_violations(&air, trace.columns(), trace.length()).is_empty());
    let pad = trace.steps();
    assert!(pad + 1 < trace.length(), "fixture needs a padding pair");
    let mut columns = trace.columns().to_vec();
    columns[SELECTOR_BASE + SEL_PADDING][pad + 1] = Felt::ZERO;
    let violations = transition_violations(&air, &columns, trace.length());
    assert!(
        violations.iter().any(|&(row, _)| row == pad),
        "expected an absorbing violation at row {pad}, got {violations:?}"
    );
}

#[test]
fn every_single_frame_trace_satisfies_the_control_flow_lookup() {
    let alpha = mock_challenge();
    for name in SINGLE_FRAME_FIXTURES {
        let (trace, rom) = trace_and_rom(&wat_from_file(name));
        let air = air_for(&trace, rom.clone());
        let length = trace.length();

        let violations = aux_violations(&air, trace.columns(), &rom, length, alpha);
        assert!(violations.is_empty(), "{name}: aux {violations:?}");

        // The grand sum opens and closes at zero: the looked-up edges are exactly the
        // ROM edges, so no forged opcode, branch, height, or offset slipped in.
        let aux = build_aux_columns(trace.columns(), &rom, length, alpha).expect("aux");
        assert_eq!(aux[1][0], Felt::ZERO, "{name}: grand sum start");
        assert_eq!(aux[1][length - 1], Felt::ZERO, "{name}: grand sum end");
    }
}

#[test]
fn forging_an_opcode_breaks_the_control_flow_lookup() {
    let alpha = mock_challenge();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, rom.clone());
    let length = trace.length();

    // Honest auxiliary witness for the honest trace.
    let honest = build_aux_columns(trace.columns(), &rom, length, alpha).expect("aux");
    let table = periodic_table(&rom, length);
    assert!(aux_residuals(&air, trace.columns(), &honest, &table, length, alpha).is_empty());

    // Forge row 0's opcode by moving its hot selector; the edge recomputed from the
    // main trace changes, but the committed multiplicities and grand sum do not, so
    // the grand-sum recurrence breaks on that row.
    let mut columns = trace.columns().to_vec();
    let hot = (0..NUM_SELECTORS)
        .find(|&j| columns[SELECTOR_BASE + j][0] == Felt::ONE)
        .expect("row 0 is one-hot");
    columns[SELECTOR_BASE + hot][0] = Felt::ZERO;
    let forged = if hot == 0 { 1 } else { 0 };
    columns[SELECTOR_BASE + forged][0] = Felt::ONE;

    let violations = aux_residuals(&air, &columns, &honest, &table, length, alpha);
    assert!(
        violations.contains(&0),
        "expected a row-0 lookup violation, got {violations:?}"
    );
}

#[test]
fn forging_a_height_breaks_the_control_flow_lookup() {
    let alpha = mock_challenge();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, rom.clone());
    let length = trace.length();

    let honest = build_aux_columns(trace.columns(), &rom, length, alpha).expect("aux");
    let table = periodic_table(&rom, length);
    assert!(aux_residuals(&air, trace.columns(), &honest, &table, length, alpha).is_empty());

    // The height rides the ROM edge, so nudging row 0's committed height packs an edge
    // absent from the table: the grand-sum recurrence breaks on that row.
    let mut columns = trace.columns().to_vec();
    columns[COL_HEIGHT][0] += Felt::ONE;

    let violations = aux_residuals(&air, &columns, &honest, &table, length, alpha);
    assert!(
        violations.contains(&0),
        "expected a row-0 lookup violation, got {violations:?}"
    );
}

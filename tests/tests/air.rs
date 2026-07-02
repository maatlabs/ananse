use ananse_air::{
    Air, NUM_AUX_CONSTRAINTS, build_aux_columns, pack_edge, periodic_table, program_rom,
};
use ananse_decoder::Module;
use ananse_executor::{Entry, OpCode, Transition, Word, execute, function_opcodes};
use ananse_lift::{Register, lift};
use ananse_tests::{
    SINGLE_FRAME_FIXTURES, TestHost, air_for, aux_residuals, aux_violations, trace_and_rom,
    trace_and_rom_entry, transition_violations, wat_from_file,
};
use ananse_trace::layout::{COL_CLK, COL_HEIGHT, COL_PC, SELECTOR_BASE, bus_slot, slot};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING, opcode_index};
use maat_field::{Felt, FieldElement};

/// A fixed stand-in for the Fiat--Shamir folding challenge the prover draws, letting
/// the auxiliary lookup be exercised without the prover.
fn mock_challenge() -> Felt {
    Felt::new(0x9e37_79b9_7f4a_7c15)
}

/// The numeric family occupies the final main transition constraints.
const NUMERIC_CONSTRAINTS: usize = 5;

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

/// Runs `export(args)`, confirms the honest trace satisfies the transition system,
/// then corrupts one limb of the operator's result on the value bus and confirms the
/// numeric family localizes the break to the operator's own row and one of its own
/// constraints. `tamper_hi` selects the high limb (the inter-limb carry / borrow and
/// the `i32` zeroing) over the low limb (the addition / subtraction balance).
fn assert_numeric_relation(
    fixture: &str,
    export: &str,
    args: &[Word],
    opcode: OpCode,
    tamper_hi: bool,
) {
    let (trace, rom) =
        trace_and_rom_entry(&wat_from_file(fixture), &Entry::Export(export.into()), args);
    let air = air_for(&trace, rom);
    assert!(
        transition_violations(&air, trace.columns(), trace.length()).is_empty(),
        "{export}{args:?}: honest trace violates the transition system"
    );

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == opcode)
        .expect("opcode present");
    // A binary operator writes its result on value-bus slot 2 of its own row; nudge
    // the chosen limb away from its correct value.
    let column = bus_slot(2) + if tamper_hi { slot::HI } else { slot::LO };
    let mut columns = trace.columns().to_vec();
    columns[column][row] += Felt::ONE;

    let numeric_base = air.num_main_transition_constraints() - NUMERIC_CONSTRAINTS;
    let violations = transition_violations(&air, &columns, trace.length());
    assert!(
        violations
            .iter()
            .any(|&(r, c)| r == row && c >= numeric_base),
        "{export}{args:?} (tamper_hi={tamper_hi}): numeric family missed the corruption, got {violations:?}"
    );
}

#[test]
fn i32_add_relation_holds_and_catches_a_wrong_result() {
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_add.wat",
            "add",
            &[Word::I32(7), Word::I32(5)],
            OpCode::I32Add,
            tamper_hi,
        );
    }
    // The carry path: the low limbs sum past 2^32 and the result wraps to zero.
    assert_numeric_relation(
        "func_add.wat",
        "add",
        &[Word::I32(u32::MAX), Word::I32(1)],
        OpCode::I32Add,
        false,
    );
}

#[test]
fn i32_sub_relation_holds_and_catches_a_wrong_result() {
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_sub.wat",
            "sub",
            &[Word::I32(7), Word::I32(5)],
            OpCode::I32Sub,
            tamper_hi,
        );
    }
    // The borrow path: the subtraction underflows and wraps to `2^32 - 2`.
    assert_numeric_relation(
        "func_sub.wat",
        "sub",
        &[Word::I32(5), Word::I32(7)],
        OpCode::I32Sub,
        false,
    );
}

#[test]
fn i64_add_relation_holds_and_catches_a_wrong_result() {
    // The low limbs carry into the high limb, so both limbs are non-trivial.
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_add_i64.wat",
            "add",
            &[Word::I64(0xFFFF_FFFF), Word::I64(1)],
            OpCode::I64Add,
            tamper_hi,
        );
    }
    // The high limbs overflow past 2^64 and the carry out is dropped.
    assert_numeric_relation(
        "func_add_i64.wat",
        "add",
        &[Word::I64(0xFFFF_FFFF_0000_0000), Word::I64(0x1_0000_0000)],
        OpCode::I64Add,
        true,
    );
}

#[test]
fn i64_sub_relation_holds_and_catches_a_wrong_result() {
    // The low limb borrows from the high limb.
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_sub_i64.wat",
            "sub",
            &[Word::I64(0x1_0000_0000), Word::I64(1)],
            OpCode::I64Sub,
            tamper_hi,
        );
    }
}

/// Every intra-body control-flow edge an execution actually takes must be a member
/// of the program ROM the verifier reconstructs; otherwise the lookup that binds
/// the trace to the module could not close. Terminal transitions (return, host
/// exit) leave the body and are handled by the AIR's halt gating, not here.
#[test]
fn program_rom_contains_every_executed_edge() {
    let mut checked = 0usize;
    for name in SINGLE_FRAME_FIXTURES {
        let module = Module::decode(&wat_from_file(name)).expect("decode");
        let program = lift(&module).expect("lift");
        let mut host = TestHost::default();
        let mut records = Vec::new();
        execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");

        let func_index = records.first().expect("fixture executes").func_index;
        let function = program
            .functions
            .iter()
            .find(|f| f.func_index == func_index)
            .expect("executed function was lifted");
        let opcodes = function_opcodes(&module, func_index).expect("opcodes");
        let rom = program_rom(&opcodes, function).expect("rom");

        // The local/global offset the operator touches, resolved the way the ROM and
        // trace resolve it: a local keeps its index, a global sits above the locals.
        let immediate_offset = |pc: usize| -> u32 {
            let schedule = &function.instrs[pc];
            schedule
                .reads
                .iter()
                .chain(&schedule.writes)
                .find_map(|reg| match reg {
                    Register::Local(index) => Some(*index),
                    Register::Global(index) => Some(function.locals_count + index),
                    Register::Stack(_) => None,
                })
                .unwrap_or(0)
        };

        for record in &records {
            if let Transition::Next(next_pc) = record.transition {
                let pc = record.pc as usize;
                let height = function.instrs[pc].height_in;
                let imm = immediate_offset(pc);
                let edge = pack_edge(record.pc, opcode_index(record.opcode), next_pc, height, imm)
                    .expect("executed edge packs");
                assert!(
                    rom.contains(&edge),
                    "{name}: edge pc{} -> {next_pc} absent from ROM",
                    record.pc
                );
                checked += 1;
            }
        }

        // The halt self-loop the padded trace tail rests on is a table member too,
        // packed at the exit sentinel's height and offset zero.
        let halt = u32::try_from(function.instrs.len()).expect("body fits u32");
        let halt_loop = pack_edge(halt, SEL_PADDING, halt, 0, 0).expect("halt edge packs");
        assert!(rom.contains(&halt_loop), "{name}: halt self-loop absent");
    }
    assert!(checked > 0, "no executed edges were checked");
}

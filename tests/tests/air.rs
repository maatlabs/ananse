use ananse_air::{pack_edge, program_rom};
use ananse_decoder::Module;
use ananse_executor::{Entry, OpCode, Transition, Word, execute, function_opcodes};
use ananse_lift::{Register, lift};
use ananse_tests::{
    SINGLE_FRAME_FIXTURES, TestHost, air_for, failing_rows, main_matrix, mock_challenges,
    permutation_of, trace_and_rom, trace_and_rom_entry, wat_from_file,
};
use ananse_trace::Trace;
use ananse_trace::layout::{
    BUS_SLOTS, COL_CLK, COL_HEIGHT, SELECTOR_BASE, bus_slot, slot, sorted, sorted_slot,
};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING, opcode_index};
use p3_field::PrimeCharacteristicRing;
use p3_goldilocks::Goldilocks as Felt;

/// The pop-two-push-one arithmetic operators, whose rows the address-binding family
/// reads the height on. A tamper meant to isolate another family avoids their rows.
fn is_arithmetic(opcode: OpCode) -> bool {
    matches!(
        opcode,
        OpCode::I32Add | OpCode::I32Sub | OpCode::I64Add | OpCode::I64Sub
    )
}

/// Runs `export(args)`, confirms the honest trace satisfies the AIR, then corrupts one
/// limb of the operator's result on the value bus and confirms the break localizes to
/// the operator's own row.
fn assert_numeric_relation(
    fixture: &str,
    export: &str,
    args: &[Word],
    opcode: OpCode,
    tamper_hi: bool,
) {
    let challenges = mock_challenges();
    let (trace, rom) =
        trace_and_rom_entry(&wat_from_file(fixture), &Entry::Export(export.into()), args);
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(
        failing_rows(&air, &main, &perm, challenges).is_empty(),
        "{export}{args:?}: honest trace violates the AIR"
    );

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == opcode)
        .expect("opcode present");
    // A binary operator writes its result on value-bus slot 2 of its own row; nudge the
    // chosen limb away from its correct value.
    let column = bus_slot(2) + if tamper_hi { slot::HI } else { slot::LO };
    let mut columns = trace.columns().to_vec();
    columns[column][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "{export}{args:?} (tamper_hi={tamper_hi}): numeric family missed the corruption, got {failing:?}"
    );
}

/// Non-arithmetic real step carrying an active value-bus read, as `(row, slot_base)`.
fn non_arithmetic_bus_read(trace: &Trace) -> Option<(usize, usize)> {
    let columns = trace.columns();
    (0..trace.steps())
        .filter(|&r| !is_arithmetic(trace.records()[r].opcode))
        .find_map(|r| {
            (0..BUS_SLOTS).find_map(|s| {
                let base = bus_slot(s);
                let active = columns[base + slot::ACTIVE][r] == Felt::ONE;
                let is_read = columns[base + slot::IS_WRITE][r] == Felt::ZERO;
                (active && is_read).then_some((r, base))
            })
        })
}

/// Active address-sorted-log entry, as `(row, entry_base)`.
fn active_sorted_entry(trace: &Trace) -> Option<(usize, usize)> {
    let columns = trace.columns();
    (0..trace.length()).find_map(|r| {
        (0..BUS_SLOTS).find_map(|s| {
            let base = sorted_slot(s);
            (columns[base + sorted::ACTIVE][r] == Felt::ONE).then_some((r, base))
        })
    })
}

#[test]
fn every_single_frame_trace_satisfies_the_constraints() {
    let challenges = mock_challenges();
    for name in SINGLE_FRAME_FIXTURES {
        let (trace, rom) = trace_and_rom(&wat_from_file(name));
        let air = air_for(&trace, &rom);
        let main = main_matrix(trace.columns(), trace.length());
        let perm = permutation_of(&main, &rom, challenges);
        let failing = failing_rows(&air, &main, &perm, challenges);
        assert!(failing.is_empty(), "{name}: {failing:?}");
    }
}

#[test]
fn adding_a_second_hot_selector_breaks_one_hotness() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // Forcing a second hot selector on row 0---the padding selector, otherwise zero on
    // a real step---makes the row's selectors sum to two and breaks one-hotness.
    let mut columns = trace.columns().to_vec();
    columns[SELECTOR_BASE + SEL_PADDING][0] = Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 violation, got {failing:?}"
    );
}

#[test]
fn stalling_the_clock_breaks_the_increment() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // The clock advances by one every row; stalling it on row 1 makes row 0's
    // increment constraint fail, so timestamps cannot be reused across accesses.
    let mut columns = trace.columns().to_vec();
    columns[COL_CLK][1] = columns[COL_CLK][0];
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 clock violation, got {failing:?}"
    );
}

#[test]
fn clearing_padding_in_the_halt_suffix_breaks_absorption() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // Padding is absorbing: clearing the padding selector on a row inside the halt
    // suffix makes the preceding still-padding row's absorbing constraint fail, so a
    // prover cannot revive a real step in the trace tail.
    let pad = trace.steps();
    assert!(pad + 1 < trace.length(), "fixture needs a padding pair");
    let mut columns = trace.columns().to_vec();
    columns[SELECTOR_BASE + SEL_PADDING][pad + 1] = Felt::ZERO;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&pad),
        "expected an absorbing violation at row {pad}, got {failing:?}"
    );
}

#[test]
fn forging_an_opcode_breaks_the_control_flow_lookup() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // Move row 0's hot selector to a different opcode: one-hotness still holds, but the
    // edge recomputed from the main trace changes, so the honest grand sum no longer
    // telescopes and the lookup recurrence breaks on that row.
    let mut columns = trace.columns().to_vec();
    let hot = (0..NUM_SELECTORS)
        .find(|&j| columns[SELECTOR_BASE + j][0] == Felt::ONE)
        .expect("row 0 is one-hot");
    columns[SELECTOR_BASE + hot][0] = Felt::ZERO;
    let forged_selector = if hot == 0 { 1 } else { 0 };
    columns[SELECTOR_BASE + forged_selector][0] = Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 lookup violation, got {failing:?}"
    );
}

#[test]
fn forging_a_height_breaks_the_control_flow_lookup() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // The height rides the ROM edge. Nudging it on a non-first, non-arithmetic step
    // packs an edge absent from the table without tripping the first-row height
    // boundary or the address family, isolating the break to the lookup recurrence.
    let row = (1..trace.steps())
        .find(|&r| !is_arithmetic(trace.records()[r].opcode))
        .expect("a non-arithmetic step past the first");
    let mut columns = trace.columns().to_vec();
    columns[COL_HEIGHT][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} lookup violation, got {failing:?}"
    );
}

#[test]
fn forging_a_bus_access_value_breaks_the_permutation() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let (row, base) = non_arithmetic_bus_read(&trace).expect("a non-arithmetic bus read");
    let mut columns = trace.columns().to_vec();
    columns[base + slot::LO][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} permutation violation, got {failing:?}"
    );
}

#[test]
fn dropping_a_bus_access_breaks_the_permutation() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let (row, base) = non_arithmetic_bus_read(&trace).expect("a non-arithmetic bus read");
    let mut columns = trace.columns().to_vec();
    columns[base + slot::ACTIVE][row] = Felt::ZERO;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} permutation violation, got {failing:?}"
    );
}

#[test]
fn forging_a_sorted_timestamp_breaks_the_permutation() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let (row, base) = active_sorted_entry(&trace).expect("an active sorted entry");
    let mut columns = trace.columns().to_vec();
    columns[base + sorted::TS][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} permutation violation, got {failing:?}"
    );
}

#[test]
fn an_access_on_the_terminal_padding_row_is_rejected() {
    let challenges = mock_challenges();
    let (trace, rom) = trace_and_rom(&wat_from_file("func_add.wat"));
    let air = air_for(&trace, &rom);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let last = trace.length() - 1;
    let mut columns = trace.columns().to_vec();
    columns[bus_slot(0) + slot::ACTIVE][last] = Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&last),
        "expected a row-{last} padding-gate violation, got {failing:?}"
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

/// Every intra-body control-flow edge an execution actually takes must be a member of
/// the program ROM the verifier reconstructs; otherwise the lookup that binds the trace
/// to the module could not close. Terminal transitions (return, host exit) leave the
/// body and are handled by the AIR's halt gating, not here.
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

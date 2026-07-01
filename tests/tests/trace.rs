use ananse_decoder::Module;
use ananse_executor::{Entry, NoHost, OpCode, Transition, Word, execute};
use ananse_lift::lift;
use ananse_tests::{SINGLE_FRAME_FIXTURES, TestHost, wat_from_file, wat_from_str};
use ananse_trace::layout::{COL_MEM_ADDR, COL_MEM_IS_WRITE, COL_MEM_VAL, COL_PC, SELECTOR_BASE};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
use ananse_trace::{Trace, TraceError};
use maat_field::{Felt, FieldElement};

/// Decodes, lifts, executes from the automatic entry point, and builds the trace.
fn trace_of(bytes: &[u8]) -> Trace {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
    let mut host = TestHost::default();
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");
    Trace::build(&program, records).expect("build")
}

fn column(trace: &Trace, index: usize) -> &[Felt] {
    trace.column_at(index).expect("column in range")
}

/// Exactly one selector column is one on every row, and the selector block holds
/// only zeros and ones.
fn assert_selectors_one_hot(trace: &Trace, name: &str) {
    for row in 0..trace.length() {
        let hot = (0..NUM_SELECTORS)
            .filter(|&sel| {
                let value = column(trace, SELECTOR_BASE + sel)[row];
                assert!(
                    value == Felt::ZERO || value == Felt::ONE,
                    "{name}: selector {sel} row {row} is neither 0 nor 1"
                );
                value == Felt::ONE
            })
            .count();
        assert_eq!(hot, 1, "{name}: row {row} is not one-hot");
    }
}

/// Each block's register columns hold, on its own row, the values the operator
/// read---the load-bearing agreement between the register bank and the schedule.
fn assert_reads_match_register_columns(trace: &Trace, name: &str) {
    for (row, record) in trace.records().iter().enumerate() {
        for read in &record.reads {
            let col = trace
                .register_column(read.reg)
                .expect("read register in range");
            assert_eq!(
                column(trace, col)[row],
                read.value.to_felt(),
                "{name}: read {:?} at row {row}",
                read.reg
            );
        }
    }
}

/// The program counter of the next row is the successor the transition selected;
/// a fall-through advances it by one, a branch to its taken target.
fn assert_pc_follows_transitions(trace: &Trace, name: &str) {
    let pc = column(trace, COL_PC);
    for (row, record) in trace.records().iter().enumerate() {
        if let Transition::Next(target) = record.transition {
            assert!(
                row + 1 < trace.steps(),
                "{name}: dangling Next at row {row}"
            );
            assert_eq!(
                pc[row + 1],
                Felt::new(u64::from(target)),
                "{name}: pc after row {row}"
            );
        }
    }
}

/// The access log is sorted by `(address, step)`.
fn assert_access_log_sorted(trace: &Trace, name: &str) {
    for pair in trace.access_log().windows(2) {
        assert!(
            (pair[0].address, pair[0].step) <= (pair[1].address, pair[1].step),
            "{name}: access log out of order"
        );
    }
}

#[test]
fn single_frame_fixtures_satisfy_trace_invariants() {
    for name in SINGLE_FRAME_FIXTURES {
        let trace = trace_of(&wat_from_file(name));
        assert_selectors_one_hot(&trace, name);
        assert_reads_match_register_columns(&trace, name);
        assert_pc_follows_transitions(&trace, name);
        assert_access_log_sorted(&trace, name);
        assert!(trace.length().is_power_of_two(), "{name}: length not 2^k");
        assert!(
            trace.length() >= trace.steps().max(8),
            "{name}: length below floor"
        );
        assert_eq!(trace.width(), 108 + trace.register_width(), "{name}: width");
    }
}

#[test]
fn recursive_and_cross_function_calls_are_unsupported() {
    // `fib(5)` recurses past its base case; `call_doubler` calls a defined
    // helper unconditionally. Both cross a defined-function call frame.
    let cases: &[(&str, Entry, &[Word])] = &[
        (
            "fibonacci.wat",
            Entry::Export("fib".into()),
            &[Word::I32(5)],
        ),
        ("func_call.wat", Entry::Auto, &[]),
    ];
    for (name, entry, args) in cases {
        let module = Module::decode(&wat_from_file(name)).expect("decode");
        let program = lift(&module).expect("lift");
        let mut host = TestHost::default();
        let mut records = Vec::new();
        execute(&module, entry, args, &mut host, &mut records).expect("execute");
        assert!(
            matches!(
                Trace::build(&program, records),
                Err(TraceError::UnsupportedCall)
            ),
            "{name}: expected UnsupportedCall"
        );
    }
}

#[test]
fn entryless_module_produces_no_trace() {
    let module = Module::decode(&wat_from_file("memory.wat")).expect("decode");
    let program = lift(&module).expect("lift");
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut NoHost, &mut records).expect("execute");
    assert!(records.is_empty());
    assert!(matches!(
        Trace::build(&program, records),
        Err(TraceError::EmptyExecution)
    ));
}

#[test]
fn store_then_load_round_trips_through_the_access_log() {
    let trace = trace_of(&wat_from_str(
        "(module (memory 1) (func (result i32)
            (i32.store (i32.const 8) (i32.const 123))
            (i32.load (i32.const 8))))",
    ));

    let log = trace.access_log();
    assert_eq!(log.len(), 2);
    assert_eq!(
        (log[0].address, log[0].value, log[0].is_write),
        (8, 123, true)
    );
    assert_eq!(
        (log[1].address, log[1].value, log[1].is_write),
        (8, 123, false)
    );
    assert!(log[0].step < log[1].step);

    let store_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I32Store)
        .expect("store present");
    let load_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I32Load)
        .expect("load present");
    assert_eq!(column(&trace, COL_MEM_IS_WRITE)[store_row], Felt::ONE);
    assert_eq!(column(&trace, COL_MEM_ADDR)[store_row], Felt::new(8));
    assert_eq!(column(&trace, COL_MEM_VAL)[store_row], Felt::new(123));
    assert_eq!(column(&trace, COL_MEM_IS_WRITE)[load_row], Felt::ZERO);
    assert_eq!(column(&trace, COL_MEM_VAL)[load_row], Felt::new(123));
}

#[test]
fn padding_holds_the_halt_state_to_a_power_of_two() {
    let trace = trace_of(&wat_from_file("i32_const.wat"));
    assert!(trace.length().is_power_of_two());
    assert!(
        trace.steps() < trace.length(),
        "fixture should need padding"
    );

    let held_pc = column(&trace, COL_PC)[trace.steps() - 1];
    let first_pad = trace.steps();
    for row in first_pad..trace.length() {
        assert_eq!(column(&trace, SELECTOR_BASE + SEL_PADDING)[row], Felt::ONE);
        assert_eq!(column(&trace, COL_PC)[row], held_pc);
        assert_eq!(column(&trace, COL_MEM_ADDR)[row], Felt::ZERO);
        assert_eq!(column(&trace, COL_MEM_VAL)[row], Felt::ZERO);
        assert_eq!(column(&trace, COL_MEM_IS_WRITE)[row], Felt::ZERO);
    }
    // Every padding row is identical: the state is held, not evolving.
    for col in 0..trace.width() {
        for row in (first_pad + 1)..trace.length() {
            assert_eq!(column(&trace, col)[row], column(&trace, col)[first_pad]);
        }
    }
}

#[test]
fn add_register_bank_carries_operands_and_result() {
    let module = Module::decode(&wat_from_file("func_add.wat")).expect("decode");
    let program = lift(&module).expect("lift");
    let mut records = Vec::new();
    execute(
        &module,
        &Entry::Export("add".into()),
        &[Word::I32(7), Word::I32(5)],
        &mut NoHost,
        &mut records,
    )
    .expect("execute");
    let trace = Trace::build(&program, records).expect("build");

    let add_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I32Add)
        .expect("add present");

    // The operands 7 and 5 sit in the add's read columns on its own row.
    let record = trace.records()[add_row].clone();
    let operands = record
        .reads
        .iter()
        .map(|read| column(&trace, trace.register_column(read.reg).unwrap())[add_row])
        .collect::<Vec<_>>();
    assert_eq!(operands, vec![Felt::new(5), Felt::new(7)]);

    // The sum 12 lands in the add's write column on the next row.
    let write = record.writes.first().copied().expect("add writes a result");
    assert_eq!(write.value, Word::I32(12));
    let col = trace.register_column(write.reg).unwrap();
    assert_eq!(column(&trace, col)[add_row + 1], Felt::new(12));
}

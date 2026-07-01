use ananse_decoder::Module;
use ananse_executor::{
    Entry, ExecuteError, MemAccess, NoHost, OpCode, StepRecord, Trap, Word, execute,
};
use ananse_lift::{Register, lift};
use ananse_tests::{TestHost, WAT_FILES, WAT_SNIPPETS, wat_from_file, wat_from_str};
use maat_field::{Felt, FieldElement};

/// Runs a module from its automatic entry point, collecting the record stream.
fn records(bytes: &[u8]) -> Vec<StepRecord> {
    let module = Module::decode(bytes).expect("decode");
    let mut host = TestHost::default();
    let mut stream = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut host, &mut stream).expect("execute");
    stream
}

/// Runs an exported function with the given arguments, returning its results.
fn run_export(bytes: &[u8], name: &str, args: &[Word]) -> Vec<Word> {
    let module = Module::decode(bytes).expect("decode");
    let mut host = TestHost::default();
    execute(
        &module,
        &Entry::Export(name.into()),
        args,
        &mut host,
        &mut (),
    )
    .expect("execute")
    .returns
}

/// Snippets that run to completion (everything but the unconditional trap).
fn completing_snippets() -> impl Iterator<Item = (&'static str, Vec<u8>)> {
    WAT_SNIPPETS
        .iter()
        .filter(|(name, _)| *name != "unreachable_op")
        .map(|(name, wat)| (*name, wat_from_str(wat)))
}

#[test]
fn execution_is_deterministic() {
    for name in WAT_FILES {
        let bytes = wat_from_file(name);
        assert_eq!(records(&bytes), records(&bytes), "{name}");
    }
    for (name, bytes) in completing_snippets() {
        assert_eq!(records(&bytes), records(&bytes), "{name}");
    }
}

#[test]
fn records_agree_with_lift_schedule() {
    let mut checked = 0usize;
    for name in WAT_FILES {
        let bytes = wat_from_file(name);
        let module = Module::decode(&bytes).expect("decode");
        let program = lift(&module).expect("lift");
        for record in records(&bytes) {
            let lifted = program
                .functions
                .iter()
                .find(|f| f.func_index == record.func_index)
                .expect("executed function was lifted");
            let scheduled = &lifted.instrs[record.pc as usize];
            let read_regs: Vec<Register> = record.reads.iter().map(|r| r.reg).collect();
            let write_regs: Vec<Register> = record.writes.iter().map(|w| w.reg).collect();
            assert_eq!(
                read_regs, scheduled.reads,
                "{name} fn{} pc{}: read registers",
                record.func_index, record.pc
            );
            assert_eq!(
                write_regs, scheduled.writes,
                "{name} fn{} pc{}: write registers",
                record.func_index, record.pc
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no records were cross-checked");
}

#[test]
fn arithmetic_and_calls_compute_expected_values() {
    assert_eq!(
        run_export(
            &wat_from_file("func_add.wat"),
            "add",
            &[Word::I32(3), Word::I32(4)]
        ),
        vec![Word::I32(7)]
    );
    assert_eq!(
        run_export(
            &wat_from_file("func_sub.wat"),
            "sub",
            &[Word::I32(7), Word::I32(3)]
        ),
        vec![Word::I32(4)]
    );
    assert_eq!(
        run_export(
            &wat_from_file("func_lts.wat"),
            "lts",
            &[Word::I32(2), Word::I32(5)]
        ),
        vec![Word::I32(1)]
    );
    assert_eq!(
        run_export(
            &wat_from_file("func_lts.wat"),
            "lts",
            &[Word::I32(5), Word::I32(2)]
        ),
        vec![Word::I32(0)]
    );
    // call_doubler(21) calls $double, which returns 21 + 21.
    assert_eq!(
        run_export(
            &wat_from_file("func_call.wat"),
            "call_doubler",
            &[Word::I32(21)]
        ),
        vec![Word::I32(42)]
    );
    // Recursive fib: fib(10) = 89 under the fixture's `n < 2 => 1` base case.
    assert_eq!(
        run_export(&wat_from_file("fibonacci.wat"), "fib", &[Word::I32(10)]),
        vec![Word::I32(89)]
    );
    assert_eq!(
        run_export(&wat_from_file("i32_const.wat"), "i32_const", &[]),
        vec![Word::I32(42)]
    );
    assert_eq!(
        run_export(&wat_from_file("local_set.wat"), "local_set", &[]),
        vec![Word::I32(42)]
    );
}

#[test]
fn branch_carries_block_result() {
    let wat =
        "(module (func (export \"f\") (result i32) (block (result i32) (br 0 (i32.const 5)))))";
    assert_eq!(run_export(&wat_from_str(wat), "f", &[]), vec![Word::I32(5)]);
}

#[test]
fn division_by_zero_traps() {
    let module = Module::decode(&wat_from_str(
        "(module (func (export \"f\") (result i32) (i32.const 1) (i32.const 0) (i32.div_s)))",
    ))
    .expect("decode");
    let err = execute(
        &module,
        &Entry::Export("f".into()),
        &[],
        &mut NoHost,
        &mut (),
    )
    .unwrap_err();
    assert_eq!(err, ExecuteError::Trap(Trap::DivideByZero));
}

#[test]
fn signed_division_overflow_traps() {
    let module = Module::decode(&wat_from_str(
        "(module (func (export \"f\") (result i32) (i32.const -2147483648) (i32.const -1) (i32.div_s)))",
    ))
    .expect("decode");
    let err = execute(
        &module,
        &Entry::Export("f".into()),
        &[],
        &mut NoHost,
        &mut (),
    )
    .unwrap_err();
    assert_eq!(err, ExecuteError::Trap(Trap::IntegerOverflow));
}

#[test]
fn unreachable_traps() {
    let module = Module::decode(&wat_from_str(
        "(module (func (export \"f\") (result i32) (unreachable)))",
    ))
    .expect("decode");
    let err = execute(
        &module,
        &Entry::Export("f".into()),
        &[],
        &mut NoHost,
        &mut (),
    )
    .unwrap_err();
    assert_eq!(err, ExecuteError::Trap(Trap::Unreachable));
}

#[test]
fn field_encoding_splits_values_into_faithful_limbs() {
    let i32_neg = run_export(
        &wat_from_str("(module (func (export \"f\") (result i32) (i32.const -1)))"),
        "f",
        &[],
    );
    assert_eq!(i32_neg, vec![Word::I32(u32::MAX)]);
    // An `i32` occupies the low limb alone; the high limb is zero.
    assert_eq!(
        i32_neg[0].to_limbs(),
        (Felt::new(u64::from(u32::MAX)), Felt::ZERO)
    );

    let i64_neg = run_export(
        &wat_from_str("(module (func (export \"f\") (result i64) (i64.const -1)))"),
        "f",
        &[],
    );
    assert_eq!(i64_neg, vec![Word::I64(u64::MAX)]);
    // `i64` -1 (`0xFFFF_FFFF_FFFF_FFFF`) exceeds the Goldilocks prime, so a
    // single residue would alias it to `0xFFFF_FFFE`. The two limbs preserve it
    // in full: `0xFFFF_FFFF + 0xFFFF_FFFF * 2^32` reconstructs the true value.
    assert_eq!(
        i64_neg[0].to_limbs(),
        (Felt::new(0xFFFF_FFFF), Felt::new(0xFFFF_FFFF))
    );

    // The high limb distinguishes the two: an `i32` -1 has a zero high limb, an
    // `i64` -1 a saturated one.
    assert_ne!(i32_neg[0].to_limbs().1, i64_neg[0].to_limbs().1);
}

#[test]
fn store_emits_a_memory_access() {
    let store = records(&wat_from_file("i32_store.wat"))
        .into_iter()
        .find(|r| r.opcode == OpCode::I32Store)
        .expect("store executed");
    assert_eq!(
        store.memory,
        vec![MemAccess {
            address: 0,
            width: 4,
            value: 42,
            store: true,
        }]
    );
}

#[test]
fn hello_world_writes_its_journal() {
    let module = Module::decode(&wat_from_file("hello_world.wat")).expect("decode");
    let mut host = TestHost::default();
    let execution = execute(&module, &Entry::Auto, &[], &mut host, &mut ()).expect("execute");
    assert_eq!(host.journal, b"Hello, World!\n");
    assert_eq!(execution.returns, vec![Word::I32(0)]);
}

#[test]
fn proc_exit_halts_with_status() {
    let wat = "(module \
        (import \"wasi_snapshot_preview1\" \"proc_exit\" (func $exit (param i32))) \
        (func (export \"_start\") (call $exit (i32.const 3))))";
    let module = Module::decode(&wat_from_str(wat)).expect("decode");
    let mut host = TestHost::default();
    let execution = execute(&module, &Entry::Auto, &[], &mut host, &mut ()).expect("execute");
    assert_eq!(execution.exit, Some(3));
    assert!(execution.returns.is_empty());
}

#[test]
fn module_without_entry_runs_nothing() {
    let module = Module::decode(&wat_from_file("memory.wat")).expect("decode");
    let mut stream: Vec<StepRecord> = Vec::new();
    let execution = execute(&module, &Entry::Auto, &[], &mut NoHost, &mut stream).expect("execute");
    assert_eq!(execution.steps, 0);
    assert!(stream.is_empty());
    assert!(execution.returns.is_empty());
}

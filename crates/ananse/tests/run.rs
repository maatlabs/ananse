//! End-to-end coverage of the `Runtime` facade and the `ananse run` binary over
//! the same fixtures, from the crate root working directory.

use std::process::Command;

use ananse::prelude::*;

const FIBONACCI: &str = "../../fixtures/fibonacci.wat";
const HELLO_WORLD: &str = "../../fixtures/hello_world.wat";

fn assemble(path: &str) -> Vec<u8> {
    wat::parse_file(path).unwrap_or_else(|e| panic!("assemble {path}: {e}"))
}

fn ananse_run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ananse"))
        .arg("run")
        .args(args)
        .output()
        .expect("spawn ananse")
}

#[test]
fn runtime_computes_recursive_fibonacci() {
    let mut runtime = Runtime::instantiate_with_wasi(&assemble(FIBONACCI)).expect("instantiate");
    let execution = runtime
        .call(&Entry::Export("fib".into()), &[Word::I32(10)])
        .expect("call fib");
    assert_eq!(execution.returns, vec![Word::I32(89)]);
}

#[test]
fn runtime_journals_hello_world_through_wasi() {
    let mut runtime = Runtime::instantiate_with_wasi(&assemble(HELLO_WORLD)).expect("instantiate");
    let execution = runtime.call(&Entry::Auto, &[]).expect("run _start");
    assert_eq!(runtime.host().journal(), b"Hello, World!\n");
    assert_eq!(execution.returns, vec![Word::I32(0)]);
}

#[test]
fn cli_prints_invoked_result_to_stdout() {
    let output = ananse_run(&[FIBONACCI, "--invoke", "fib", "10"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "89");
}

#[test]
fn cli_writes_the_guest_journal_to_stdout() {
    let output = ananse_run(&[HELLO_WORLD]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Hello, World!\n");
}

#[test]
fn cli_rejects_a_wrong_argument_count() {
    let output = ananse_run(&[FIBONACCI, "--invoke", "fib"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("argument"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_runs_the_auto_export_and_prints_its_result() {
    let output = ananse_run(&[FIBONACCI, "10"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "89");
}

#[test]
fn cli_step_summary_is_opt_in() {
    let quiet = ananse_run(&[HELLO_WORLD]);
    assert!(
        !String::from_utf8_lossy(&quiet.stderr).contains("steps"),
        "default run leaked a step summary: {}",
        String::from_utf8_lossy(&quiet.stderr)
    );
    let verbose = ananse_run(&["--verbose", HELLO_WORLD]);
    assert!(
        String::from_utf8_lossy(&verbose.stderr).contains("steps"),
        "verbose run withheld the step summary: {}",
        String::from_utf8_lossy(&verbose.stderr)
    );
}

#[test]
fn cli_adopts_the_guest_exit_status() {
    let module = std::env::temp_dir().join("ananse_cli_proc_exit.wat");
    std::fs::write(
        &module,
        "(module (import \"wasi_snapshot_preview1\" \"proc_exit\" (func $exit (param i32))) \
         (func (export \"_start\") (call $exit (i32.const 3))))",
    )
    .expect("write temp module");
    let status = Command::new(env!("CARGO_BIN_EXE_ananse"))
        .args(["run", module.to_str().expect("utf-8 temp path")])
        .status()
        .expect("spawn ananse");
    let _ = std::fs::remove_file(&module);
    assert_eq!(status.code(), Some(3));
}

use mvm_decoder::{DecodeError, ExportKind, Module, WASI_MODULE};
use mvm_tests::{wat_from_file, wat_from_str};

#[test]
fn wat_file_decodes() {
    for name in mvm_tests::WAT_FILES {
        let bytes = wat_from_file(name);
        Module::decode(&bytes)
            .unwrap_or_else(|e| panic!("fixture {name} should decode but errored: {e}"));
    }
}

#[test]
fn hello_world_extracts_wasi_import_and_start_export() {
    let bytes = wat_from_file("hello_world.wat");
    let module = Module::decode(&bytes).expect("hello_world.wat decodes");
    let imports = module.imports();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].module, WASI_MODULE);
    assert_eq!(imports[0].name, "fd_write");
    let start_export = module
        .exports()
        .iter()
        .find(|e| e.name == "_start")
        .expect("_start export present");
    assert_eq!(start_export.kind, ExportKind::Function);
}

#[test]
fn float_opcode_is_rejected() {
    let bytes = wat_from_str(
        r#"
        (module
          (func (result f32)
            (f32.const 1.0)
            (f32.const 2.0)
            f32.add))
    "#,
    );
    let result = Module::decode(&bytes);
    assert!(
        matches!(result, Err(DecodeError::ValidationFailed { .. })),
        "float opcodes must be rejected at validation, got {result:?}"
    );
}

#[test]
fn float_typed_global_is_rejected() {
    let bytes = wat_from_str(r#"(module (global f32 (f32.const 1.0)))"#);
    let result = Module::decode(&bytes);
    assert!(
        matches!(result, Err(DecodeError::ValidationFailed { .. })),
        "float-typed global must be rejected, got {result:?}"
    );
}

#[test]
fn import_from_env_is_rejected() {
    let bytes = wat_from_file("import.wat");
    match Module::decode(&bytes) {
        Err(DecodeError::ForbiddenImportModule { module }) => {
            assert_eq!(module, "env");
        }
        other => panic!("expected ForbiddenImportModule, got {other:?}"),
    }
}

#[test]
fn forbidden_wasi_function_is_rejected() {
    let bytes = wat_from_str(
        r#"
        (module
          (import "wasi_snapshot_preview1" "clock_time_get"
            (func $now (param i32 i64 i32) (result i32))))
    "#,
    );
    match Module::decode(&bytes) {
        Err(DecodeError::ForbiddenWasiImport { module, name }) => {
            assert_eq!(module, WASI_MODULE);
            assert_eq!(name, "clock_time_get");
        }
        other => panic!("expected ForbiddenWasiImport, got {other:?}"),
    }
}

#[test]
fn malformed_binary_fails_validation() {
    let bytes: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];
    match Module::decode(&bytes) {
        Err(DecodeError::ValidationFailed { .. }) | Err(DecodeError::InvalidBinary { .. }) => {}
        other => panic!("expected InvalidBinary or ValidationFailed, got {other:?}"),
    }
}

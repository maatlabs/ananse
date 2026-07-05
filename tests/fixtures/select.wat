(module
  (func (export "select32") (param i32 i32 i32) (result i32)
    (select (local.get 0) (local.get 1) (local.get 2)))
  (func (export "select64") (param i64 i64 i32) (result i64)
    (select (local.get 0) (local.get 1) (local.get 2)))
)

(module
  (func (export "and") (param i64 i64) (result i64) (i64.and (local.get 0) (local.get 1)))
  (func (export "or") (param i64 i64) (result i64) (i64.or (local.get 0) (local.get 1)))
  (func (export "xor") (param i64 i64) (result i64) (i64.xor (local.get 0) (local.get 1)))
  (func (export "and32") (param i32 i32) (result i32) (i32.and (local.get 0) (local.get 1)))
  (func (export "popcnt") (param i64) (result i64) (i64.popcnt (local.get 0)))
  (func (export "popcnt32") (param i32) (result i32) (i32.popcnt (local.get 0)))
)

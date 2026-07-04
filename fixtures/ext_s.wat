(module
  (func (export "ext_s") (param i32) (result i64)
    (i64.extend_i32_s (local.get 0))))

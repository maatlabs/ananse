(module
  (func $conv (export "conv") (param i64) (result i64)
    (i64.extend_i32_u (i32.wrap_i64 (local.get 0)))
  )
)

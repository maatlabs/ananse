(module
  (global $g (mut i32) (i32.const 0))
  (func $g_rw (export "g_rw") (param i32) (result i32)
    (global.set $g (local.get 0))
    (global.get $g)
  )
)

(module
  (func $tee (export "tee") (param i32) (result i32)
    (local $x i32)
    (local.get 0)
    (local.tee $x)
    (local.get 0)
    (drop)
  )
)

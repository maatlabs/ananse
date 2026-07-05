;; Iterative factorial over 64-bit integers.
;;
;; `factorial(n)` multiplies the running accumulator by each value from `n` down
;; to 1. A representative bounded-loop integer kernel: `factorial(20)` is the
;; largest factorial that fits in an unsigned 64-bit word.
(module
  (func $factorial (export "factorial") (param $n i64) (result i64)
    (local $acc i64)
    (local.set $acc (i64.const 1))
    (block $done
      (loop $loop
        (br_if $done (i64.eqz (local.get $n)))
        (local.set $acc (i64.mul (local.get $acc) (local.get $n)))
        (local.set $n (i64.sub (local.get $n) (i64.const 1)))
        (br $loop)
      )
    )
    (local.get $acc)
  )
)

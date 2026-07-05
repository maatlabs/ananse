;; Modular exponentiation by square-and-multiply.
;;
;; Computes `base^exp mod m`, squaring the base and conditionally multiplying it
;; into the result on each set exponent bit. This is the inner loop of RSA and
;; Diffie-Hellman; `modpow(4, 13, 497) = 445`.
(module
  (func $modpow (export "modpow")
      (param $base i64) (param $exp i64) (param $m i64) (result i64)
    (local $result i64)
    (local.set $result (i64.const 1))
    (local.set $base (i64.rem_u (local.get $base) (local.get $m)))
    (block $done
      (loop $loop
        (br_if $done (i64.eqz (local.get $exp)))
        (if (i32.wrap_i64 (i64.and (local.get $exp) (i64.const 1)))
          (then
            (local.set $result
              (i64.rem_u
                (i64.mul (local.get $result) (local.get $base))
                (local.get $m)))))
        (local.set $exp (i64.shr_u (local.get $exp) (i64.const 1)))
        (local.set $base
          (i64.rem_u
            (i64.mul (local.get $base) (local.get $base))
            (local.get $m)))
        (br $loop)
      )
    )
    (local.get $result)
  )
)

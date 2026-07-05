;; Greatest common divisor by the Euclidean algorithm.
;;
;; Repeatedly replaces `(a, b)` with `(b, a mod b)` until the remainder is zero.
;; `gcd(1071, 462) = 21`.
(module
  (func $gcd (export "gcd") (param $a i64) (param $b i64) (result i64)
    (local $t i64)
    (block $done
      (loop $loop
        (br_if $done (i64.eqz (local.get $b)))
        (local.set $t (i64.rem_u (local.get $a) (local.get $b)))
        (local.set $a (local.get $b))
        (local.set $b (local.get $t))
        (br $loop)
      )
    )
    (local.get $a)
  )
)

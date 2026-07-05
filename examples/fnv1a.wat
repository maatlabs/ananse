;; FNV-1a 64-bit hash over a byte range in linear memory.
;;
;; For each byte the accumulator is XOR-folded with the byte and multiplied by
;; the 64-bit FNV prime. The one-word compression step (`load8_u`, `xor`, `mul`)
;; is a real hash-round kernel; `fnv1a(0, 6)` hashes the six bytes of "Ananse".
(module
  (memory 1)
  (data (i32.const 0) "Ananse")
  (func $fnv1a (export "fnv1a") (param $ptr i32) (param $len i32) (result i64)
    (local $hash i64)
    (local $end i32)
    (local.set $hash (i64.const 0xcbf29ce484222325))
    (local.set $end (i32.add (local.get $ptr) (local.get $len)))
    (block $done
      (loop $loop
        (br_if $done (i32.ge_u (local.get $ptr) (local.get $end)))
        (local.set $hash
          (i64.mul
            (i64.xor (local.get $hash) (i64.load8_u (local.get $ptr)))
            (i64.const 0x100000001b3)))
        (local.set $ptr (i32.add (local.get $ptr) (i32.const 1)))
        (br $loop)
      )
    )
    (local.get $hash)
  )
)

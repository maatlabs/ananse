;; CRC-32 (IEEE 802.3, reflected) over a byte range in linear memory.
;;
;; The classic bit-at-a-time algorithm: fold each input byte into the running
;; remainder, then reduce eight times against the reflected polynomial
;; `0xEDB88320`. `crc32(0, 9)` over "123456789" yields the standard check value
;; `0xCBF43926` (3421780262).
(module
  (memory 1)
  (data (i32.const 0) "123456789")
  (func $crc32 (export "crc32") (param $ptr i32) (param $len i32) (result i32)
    (local $crc i32)
    (local $end i32)
    (local $i i32)
    (local.set $crc (i32.const 0xffffffff))
    (local.set $end (i32.add (local.get $ptr) (local.get $len)))
    (block $done
      (loop $byte
        (br_if $done (i32.ge_u (local.get $ptr) (local.get $end)))
        (local.set $crc
          (i32.xor (local.get $crc) (i32.load8_u (local.get $ptr))))
        (local.set $i (i32.const 0))
        (loop $bit
          (local.set $crc
            (if (result i32) (i32.and (local.get $crc) (i32.const 1))
              (then
                (i32.xor
                  (i32.shr_u (local.get $crc) (i32.const 1))
                  (i32.const 0xedb88320)))
              (else
                (i32.shr_u (local.get $crc) (i32.const 1)))))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br_if $bit (i32.lt_u (local.get $i) (i32.const 8)))
        )
        (local.set $ptr (i32.add (local.get $ptr) (i32.const 1)))
        (br $byte)
      )
    )
    (i32.xor (local.get $crc) (i32.const 0xffffffff))
  )
)

;; Merkle authentication-path fold.
;;
;; Starting from a leaf, repeatedly compresses the running node with the next
;; sibling hash read from linear memory, walking `depth` levels toward the root.
;; The compression function `$mix` is a two-word FNV-1a round; a verifier accepts
;; the path when the returned root matches the committed one. The bundled
;; siblings are three 64-bit words `0x11..`, `0x22..`, `0x33..`.
(module
  (memory 1)
  (data (i32.const 0)
    "\11\11\11\11\11\11\11\11"
    "\22\22\22\22\22\22\22\22"
    "\33\33\33\33\33\33\33\33")
  (func $mix (param $a i64) (param $b i64) (result i64)
    (i64.mul
      (i64.xor
        (i64.mul
          (i64.xor (i64.const 0xcbf29ce484222325) (local.get $a))
          (i64.const 0x100000001b3))
        (local.get $b))
      (i64.const 0x100000001b3)))
  (func $merkle_root (export "merkle_root")
      (param $leaf i64) (param $ptr i32) (param $depth i32) (result i64)
    (local $node i64)
    (local $i i32)
    (local.set $node (local.get $leaf))
    (block $done
      (loop $loop
        (br_if $done (i32.ge_u (local.get $i) (local.get $depth)))
        (local.set $node
          (call $mix (local.get $node) (i64.load (local.get $ptr))))
        (local.set $ptr (i32.add (local.get $ptr) (i32.const 8)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )
    (local.get $node)
  )
)

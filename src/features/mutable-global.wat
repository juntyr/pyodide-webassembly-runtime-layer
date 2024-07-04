;; Name: Mutable Global
;; Proposal: https://github.com/WebAssembly/mutable-global
;; Features: mutable-global

(module
  (import "a" "b" (global (mut i32)))
  (global (export "a") (mut i32) (i32.const 0))
)

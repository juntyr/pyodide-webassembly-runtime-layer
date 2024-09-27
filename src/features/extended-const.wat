;; wasm-as: --enable-extended-const

;; Name: Extented Const Expressesions
;; Proposal: https://github.com/WebAssembly/extended-const
;; Features: extended-const

(module
  (memory 1)
  (data (offset (i32.add (i32.const 1) (i32.const 2))))
)

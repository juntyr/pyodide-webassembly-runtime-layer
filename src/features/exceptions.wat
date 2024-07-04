;; Name: Exception handling
;; Proposal: https://github.com/WebAssembly/exception-handling
;; Features: exceptions

;; The exceptions canary is different from wasm-feature-detect,
;;  which uses the now deprecated try instruction.

(module
  (tag)
  (func
    throw 0
  )
)

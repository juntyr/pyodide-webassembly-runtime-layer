;; wasm-as: --enable-exception-handling

;; Name: Exception handling
;; Proposal: https://github.com/WebAssembly/exception-handling
;; Features: exceptions

;; The exceptions canary is different from wasm-feature-detect,
;;  which uses instructions from the reference-types proposal.

(module
  (tag)
  (func
    block
      try_table (catch 0 0)
        unreachable
      end
    end
  )
)

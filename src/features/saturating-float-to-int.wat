;; Name: Non-trapping Float-to-int Conversions
;; Proposal: https://github.com/WebAssembly/nontrapping-float-to-int-conversions
;; Features: nontrapping-float-to-int-conversion

(module
  (func
    f32.const 0
    i32.trunc_sat_f32_s
    drop
  )
)

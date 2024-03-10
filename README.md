[![CI Status]][workflow] [![MSRV]][repo] [![Latest Version]][crates.io] [![Rust Doc Crate]][docs.rs] [![Rust Doc Main]][docs]

[CI Status]: https://img.shields.io/github/actions/workflow/status/juntyr/pyodide-webassembly-runtime-layer/ci.yml?branch=main
[workflow]: https://github.com/juntyr/pyodide-webassembly-runtime-layer/actions/workflows/ci.yml?query=branch%3Amain

[MSRV]: https://img.shields.io/badge/MSRV-1.70.0-blue
[repo]: https://github.com/juntyr/pyodide-webassembly-runtime-layer

[Latest Version]: https://img.shields.io/crates/v/pyodide-webassembly-runtime-layer
[crates.io]: https://crates.io/crates/pyodide-webassembly-runtime-layer

[Rust Doc Crate]: https://img.shields.io/docsrs/pyodide-webassembly-runtime-layer
[docs.rs]: https://docs.rs/pyodide-webassembly-runtime-layer/

[Rust Doc Main]: https://img.shields.io/badge/docs-main-blue
[docs]: https://juntyr.github.io/pyodide-webassembly-runtime-layer/pyodide_webassembly_runtime_layer

# pyodide-webassembly-runtime-layer

`pyodide-webassembly-runtime-layer` implements the [`wasm_runtime_layer`] backend API to provide access to the web browser's [`WebAssembly`] runtime using [`Pyodide`].

The implementation of this crate is heavily inspired by the [`web_backend`] of the [`wasm_runtime_layer`]. Instead of relying on the [`js-sys`] and [`wasm-bindgen`] crates to generate JavaScript-based bindings to the [`WebAssembly`] JavaScript API, this crate uses [`Pyodide`]'s [`js`] FFI layer to interact with [`WebAssembly`] through Python running inside WebAssembly. `pyodide-webassembly-runtime-layer` is therefore useful when developing a Python module in Rust, e.g. using [`PyO3`], which requires access to some WebAssembly runtime using the [`wasm_runtime_layer`] API and may be deployed to the web itself using [`Pyodide`].

## Memory Management

`pyodide-webassembly-runtime-layer` generally tries to keep memory management intuitive by relying primarily on Python's reference counting to drop objects once they are no longer needed by both the user-written Rust code and the [`WebAssembly`] runtime. As this crate coordinates interop across Rust, Python, and JavaScript, it takes extra care to avoid reference cycles across the different memory management strategies of the languages which would otherwise lead to memory leakage. If using this crate produces a memory leak that is avoided with a different [`wasm_runtime_layer`] backend, please [report it as a bug][new-issue].

There is one exception to the intuitive memory management strategy:

- [`Func::new`] creates a host function, which may capture arbitrary data. To avoid cross-language reference cycles, it is stored using [`wobbly`] references inside the [`Func`] and its associated [`Store`]. Even though the host function and its data are dropped once either the [`Store`] is dropped or references to the [`Func`] are dropped, additional bookkeeping data is required until both have been dropped.

[`wasm_runtime_layer`]: https://docs.rs/wasm_runtime_layer/0.2/
[`WebAssembly`]: https://developer.mozilla.org/en-US/docs/WebAssembly
[`Pyodide`]: https://pyodide.org/en/stable/
[`web_backend`]: https://github.com/DouglasDwyer/wasm_runtime_layer/tree/5d4360daedb9aa86529b6301b8580a7230908c86/src/backend/backend_web
[`js-sys`]: https://docs.rs/js-sys/
[`wasm-bindgen`]: https://docs.rs/wasm-bindgen/
[`js`]: https://pyodide.org/en/stable/usage/api/python-api.html
[`PyO3`]: https://docs.rs/pyo3/0.20/
[new-issue]: https://github.com/juntyr/pyodide-webassembly-runtime-layer/issues/new
[`Func::new`]: https://docs.rs/wasm_runtime_layer/0.2/wasm_runtime_layer/struct.Func.html#method.new
[`wobbly`]: https://docs.rs/wobbly/0.1/
[`Func`]: https://docs.rs/wasm_runtime_layer/0.2/wasm_runtime_layer/struct.Func.html
[`Store`]: https://docs.rs/wasm_runtime_layer/0.2/wasm_runtime_layer/struct.Store.html

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Funding

`pyodide-webassembly-runtime-layer` has been developed as part of [ESiWACE3](https://www.esiwace.eu), the third phase of the Centre of Excellence in Simulation of Weather and Climate in Europe.

Funded by the European Union. This work has received funding from the European High Performance Computing Joint Undertaking (JU) under grant agreement No 101093054.

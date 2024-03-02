#![deny(clippy::complexity)]
#![deny(clippy::correctness)]
#![warn(clippy::nursery)]
#![warn(clippy::pedantic)]
#![deny(clippy::perf)]
#![deny(clippy::style)]
#![deny(clippy::suspicious)]
#![warn(missing_docs)]

//! [![CI Status]][workflow] [![MSRV]][repo] [![Latest Version]][crates.io] [![Rust Doc Crate]][docs.rs] [![Rust Doc Main]][docs]
//!
//! [CI Status]: https://img.shields.io/github/actions/workflow/status/juntyr/pyodide-webassembly-runtime-layer/ci.yml?branch=main
//! [workflow]: https://github.com/juntyr/pyodide-webassembly-runtime-layer/actions/workflows/ci.yml?query=branch%3Amain
//!
//! [MSRV]: https://img.shields.io/badge/MSRV-1.70.0-blue
//! [repo]: https://github.com/juntyr/pyodide-webassembly-runtime-layer
//!
//! [Latest Version]: https://img.shields.io/crates/v/pyodide-webassembly-runtime-layer
//! [crates.io]: https://crates.io/crates/pyodide-webassembly-runtime-layer
//!
//! [Rust Doc Crate]: https://img.shields.io/docsrs/pyodide-webassembly-runtime-layer
//! [docs.rs]: https://docs.rs/pyodide-webassembly-runtime-layer/
//!
//! [Rust Doc Main]: https://img.shields.io/badge/docs-main-blue
//! [docs]: https://juntyr.github.io/pyodide-webassembly-runtime-layer/pyodide_webassembly_runtime_layer
//!
//! `pyodide-webassembly-runtime-layer` implements the `wasm_runtime_layer` API to
//! provide access to the web browser's `WebAssembly` runtime using `pyodide`.

use wasm_runtime_layer::backend::WasmEngine;

/// Conversion to and from Python
mod conversion;
/// Extern host references
mod externref;
/// Functions
mod func;
/// Globals
mod global;
/// Instances
mod instance;
/// Memories
mod memory;
/// WebAssembly modules
mod module;
/// Stores all the WebAssembly state for a given collection of modules with a similar lifetime
mod store;
/// WebAssembly tables
mod table;

pub use externref::ExternRef;
pub use func::Func;
pub use global::Global;
pub use instance::Instance;
pub use memory::Memory;
pub use module::Module;
pub use store::{Store, StoreContext, StoreContextMut};
pub use table::Table;

#[derive(Default, Debug, Clone)]
/// Runtime for WebAssembly
pub struct Engine {
    _private: (),
}

impl WasmEngine for Engine {
    type ExternRef = ExternRef;
    type Func = Func;
    type Global = Global;
    type Instance = Instance;
    type Memory = Memory;
    type Module = Module;
    type Store<T> = Store<T>;
    type StoreContext<'a, T: 'a> = StoreContext<'a, T>;
    type StoreContextMut<'a, T: 'a> = StoreContextMut<'a, T>;
    type Table = Table;
}

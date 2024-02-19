#![deny(clippy::complexity)]
#![deny(clippy::correctness)]
#![warn(clippy::nursery)]
#![warn(clippy::pedantic)]
#![deny(clippy::perf)]
#![deny(clippy::style)]
#![deny(clippy::suspicious)]
#![warn(missing_docs)]

//! `pyodide_wasm_runtime_layer` implements the `wasm_runtime_layer` API to
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

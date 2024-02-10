use pyo3::prelude::*;
use wasm_runtime_layer::backend::{AsContextMut, WasmEngine, WasmExternRef};

use crate::{conversion::ToPy, Engine};

#[derive(Debug, Clone)]
/// Extern host reference type
pub struct ExternRef {
    _private: (),
}

impl WasmExternRef<Engine> for ExternRef {
    fn new<T: 'static + Send + Sync>(_ctx: impl AsContextMut<Engine>, _object: Option<T>) -> Self {
        unimplemented!("ExternRef is not supported in the pyodide backend")
    }

    fn downcast<'a, T: 'static, S: 'a>(
        &self,
        _ctx: <Engine as WasmEngine>::StoreContext<'a, S>,
    ) -> anyhow::Result<Option<&'a T>> {
        unimplemented!("ExternRef is not supported in the pyodide backend")
    }
}

impl ToPy for ExternRef {
    fn to_py(&self, _py: Python) -> Py<PyAny> {
        unimplemented!("ExternRef is not supported in the pyodide backend")
    }
}

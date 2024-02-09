use pyo3::{intern, prelude::*, types::PyDict};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmGlobal},
    GlobalType,
};

use crate::{
    conversion::{instanceof, ToPy},
    Engine, ValueExt, ValueTypeExt,
};

/// A global variable accesible as an import or export in a module
///
/// Stored within the store
#[derive(Debug, Clone)]
pub struct Global {
    /// The global value
    value: Py<PyAny>,
    /// The global type
    ty: GlobalType,
}

impl WasmGlobal<Engine> for Global {
    fn new(_ctx: impl AsContextMut<Engine>, value: Value<Engine>, mutable: bool) -> Self {
        Python::with_gil(|py| -> Result<Self, PyErr> {
            let ty = GlobalType::new(value.ty(), mutable);

            let desc = PyDict::new(py);
            desc.set_item(intern!(py, "value"), value.ty().as_js_descriptor())?;
            desc.set_item(intern!(py, "mutable"), mutable)?;

            let value = value.to_py(py);

            let global = web_assembly_global(py)?
                .getattr(intern!(py, "new"))?
                .call1((desc, value))?;

            Ok(Self {
                ty,
                value: global.into_py(py),
            })
        })
        .unwrap()
    }

    fn ty(&self, _ctx: impl AsContext<Engine>) -> GlobalType {
        self.ty
    }

    fn set(&self, _ctx: impl AsContextMut<Engine>, new_value: Value<Engine>) -> anyhow::Result<()> {
        if !self.ty.mutable() {
            return Err(anyhow::anyhow!("Global is not mutable"));
        }

        Python::with_gil(|py| {
            let global = self.value.as_ref(py);

            let new_value = new_value.to_py(py);

            global.setattr(intern!(py, "value"), new_value)?;

            Ok(())
        })
    }

    fn get(&self, mut ctx: impl AsContextMut<Engine>) -> Value<Engine> {
        Python::with_gil(|py| {
            let global = self.value.as_ref(py);

            let value = global.getattr(intern!(py, "value"))?;

            Value::from_py_typed(&mut ctx.as_context_mut(), &self.ty.content(), value)
        })
        .unwrap()
    }
}

impl ToPy for Global {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        self.value.clone_ref(py)
    }
}

impl Global {
    #[allow(unused)] // FIXME
    /// Creates a new global from a Python value
    pub(crate) fn from_exported_global(
        value: &PyAny,
        signature: GlobalType,
    ) -> Result<Option<Self>, PyErr> {
        let py = value.py();

        if !instanceof(py, value, web_assembly_global(py)?)? {
            return Ok(None);
        }

        Ok(Some(Self {
            value: value.into_py(py),
            ty: signature,
        }))
    }
}

fn web_assembly_global(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Global"))
}

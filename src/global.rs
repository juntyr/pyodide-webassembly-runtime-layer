use pyo3::{intern, prelude::*, types::PyDict};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmGlobal},
    GlobalType,
};

use crate::{
    conversion::{instanceof, py_dict_to_js_object, ToPy, ValueExt, ValueTypeExt},
    Engine,
};

/// A global variable accesible as an import or export in a module
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
            #[cfg(feature = "tracing")]
            tracing::debug!(?value, mutable, "Global::new");

            let ty = GlobalType::new(ValueExt::ty(&value), mutable);

            let desc = PyDict::new(py);
            desc.set_item(
                intern!(py, "value"),
                ValueExt::ty(&value).as_js_descriptor(),
            )?;
            desc.set_item(intern!(py, "mutable"), mutable)?;
            let desc = py_dict_to_js_object(py, desc)?;

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
            #[cfg(feature = "tracing")]
            tracing::debug!(%global, ?self.ty, ?new_value, "Global::set");

            let new_value = new_value.to_py(py);

            global.setattr(intern!(py, "value"), new_value)?;

            Ok(())
        })
    }

    fn get(&self, _ctx: impl AsContextMut<Engine>) -> Value<Engine> {
        Python::with_gil(|py| {
            let global = self.value.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%global, ?self.ty, "Global::get");

            let value = global.getattr(intern!(py, "value"))?;

            Value::from_py_typed(value, self.ty.content())
        })
        .unwrap()
    }
}

impl ToPy for Global {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(value = %self.value, ?self.ty, "Global::to_py");

        self.value.clone_ref(py)
    }
}

impl Global {
    /// Creates a new global from a Python value
    pub(crate) fn from_exported_global(
        py: Python,
        value: Py<PyAny>,
        signature: GlobalType,
    ) -> anyhow::Result<Self> {
        if !instanceof(py, value.as_ref(py), web_assembly_global(py)?)? {
            anyhow::bail!("expected WebAssembly.Global but found {value:?}");
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(value = %value.as_ref(py), ?signature, "Global::from_exported_global");

        Ok(Self {
            value,
            ty: signature,
        })
    }
}

fn web_assembly_global(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Global"))
}

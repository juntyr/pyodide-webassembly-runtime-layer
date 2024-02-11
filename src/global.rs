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

impl Drop for Global {
    fn drop(&mut self) {
        Python::with_gil(|py| {
            let global = self.value.as_ref(py);
            let _res = global.call_method0(intern!(py, "destroy"));
            #[cfg(feature = "tracing")]
            match _res {
                Ok(ok) => tracing::debug!(?self.ty, %ok, "Global::drop"),
                Err(err) => tracing::debug!(?self.ty, %err, "Global::drop"),
            }
        })
    }
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

            // value is passed to WebAssembly global, so it must be turned into JS
            let value = value.to_py_js(py)?;

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

            // value is passed to WebAssembly global, so it must be turned into JS
            let new_value = new_value.to_py_js(py)?;

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

            Value::from_py_typed(value, &self.ty.content())
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
        value: &PyAny,
        signature: GlobalType,
    ) -> anyhow::Result<Self> {
        let py = value.py();

        if !instanceof(py, value, web_assembly_global(py)?)? {
            anyhow::bail!("expected WebAssembly.Global but found {value:?}");
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(%value, ?signature, "Global::from_exported_global");

        Ok(Self {
            value: value.into_py(py),
            ty: signature,
        })
    }
}

fn web_assembly_global(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Global"))
}

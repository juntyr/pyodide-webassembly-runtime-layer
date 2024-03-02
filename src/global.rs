use std::sync::OnceLock;

use pyo3::{intern, prelude::*};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmGlobal},
    GlobalType,
};

use crate::{
    conversion::{create_js_object, instanceof, ToPy, ValueExt, ValueTypeExt},
    Engine,
};

/// A global variable accesible as an import or export in a module.
///
/// This type wraps a [`WebAssembly.Global`] from the JavaScript API.
///
/// [`WebAssembly.Global`]: https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Global
#[derive(Debug, Clone)]
pub struct Global {
    /// The global value
    global: Py<PyAny>,
    /// The global type
    ty: GlobalType,
}

impl WasmGlobal<Engine> for Global {
    fn new(_ctx: impl AsContextMut<Engine>, value: Value<Engine>, mutable: bool) -> Self {
        Python::with_gil(|py| -> Result<Self, PyErr> {
            #[cfg(feature = "tracing")]
            tracing::debug!(?value, mutable, "Global::new");

            let ty = GlobalType::new(ValueExt::ty(&value), mutable);

            let desc = create_js_object(py)?;
            desc.setattr(
                py,
                intern!(py, "value"),
                ValueExt::ty(&value).as_js_descriptor(),
            )?;
            desc.setattr(py, intern!(py, "mutable"), mutable)?;

            let value = value.to_py(py);

            let global =
                web_assembly_global(py).call_method1(py, intern!(py, "new"), (desc, value))?;

            Ok(Self { global, ty })
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
            #[cfg(feature = "tracing")]
            tracing::debug!(global = %self.global.as_ref(py), ?self.ty, ?new_value, "Global::set");

            let new_value = new_value.to_py(py);

            self.global.setattr(py, intern!(py, "value"), new_value)?;

            Ok(())
        })
    }

    fn get(&self, _ctx: impl AsContextMut<Engine>) -> Value<Engine> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(global = %self.global.as_ref(py), ?self.ty, "Global::get");

            let value = self.global.getattr(py, intern!(py, "value"))?;

            Value::from_py_typed(py, value, self.ty.content())
        })
        .unwrap()
    }
}

impl ToPy for Global {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(value = %self.global, ?self.ty, "Global::to_py");

        self.global.clone_ref(py)
    }
}

impl Global {
    /// Creates a new global from a Python value
    pub(crate) fn from_exported_global(
        py: Python,
        global: Py<PyAny>,
        ty: GlobalType,
    ) -> anyhow::Result<Self> {
        if !instanceof(py, &global, web_assembly_global(py))? {
            anyhow::bail!(
                "expected WebAssembly.Global but found {}",
                global.as_ref(py)
            );
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(global = %global.as_ref(py), ?ty, "Global::from_exported_global");

        Ok(Self { global, ty })
    }
}

fn web_assembly_global(py: Python) -> &'static Py<PyAny> {
    static WEB_ASSEMBLY_GLOBAL: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    WEB_ASSEMBLY_GLOBAL.get_or_init(|| {
        py.import(intern!(py, "js"))
            .unwrap()
            .getattr(intern!(py, "WebAssembly"))
            .unwrap()
            .getattr(intern!(py, "Global"))
            .unwrap()
            .into_py(py)
    })
}

use pyo3::{intern, prelude::*, sync::GILOnceCell};
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
                intern!(py, "value"),
                ValueExt::ty(&value).as_js_descriptor(),
            )?;
            desc.setattr(intern!(py, "mutable"), mutable)?;

            let value = value.to_py(py);

            let global =
                web_assembly_global(py)?.call_method1(intern!(py, "new"), (desc, value))?;

            Ok(Self {
                global: global.unbind(),
                ty,
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
            let global = self.global.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(global = %global, ?self.ty, ?new_value, "Global::set");

            let new_value = new_value.to_py(py);

            global.setattr(intern!(py, "value"), new_value)?;

            Ok(())
        })
    }

    fn get(&self, _ctx: impl AsContextMut<Engine>) -> Value<Engine> {
        Python::with_gil(|py| {
            let global = self.global.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(global = %global, ?self.ty, "Global::get");

            let value = global.getattr(intern!(py, "value"))?;

            Value::from_py_typed(value, self.ty.content())
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
        global: Bound<PyAny>,
        ty: GlobalType,
    ) -> anyhow::Result<Self> {
        if !instanceof(&global, web_assembly_global(global.py())?)? {
            anyhow::bail!("expected WebAssembly.Global but found {global}",);
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(global = %global, ?ty, "Global::from_exported_global");

        Ok(Self {
            global: global.unbind(),
            ty,
        })
    }
}

fn web_assembly_global(py: Python) -> Result<&Bound<PyAny>, PyErr> {
    static WEB_ASSEMBLY_GLOBAL: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

    WEB_ASSEMBLY_GLOBAL
        .get_or_try_init(py, || {
            Ok(py
                .import_bound(intern!(py, "js"))?
                .getattr(intern!(py, "WebAssembly"))?
                .getattr(intern!(py, "Global"))?
                .into_py(py))
        })
        .map(|x| x.bind(py))
}

use std::any::Any;

use id_arena::Id;
use pyo3::prelude::*;
use wasm_runtime_layer::backend::{AsContextMut, WasmExternRef};

use crate::{
    conversion::{py_to_js_proxy, ToPy},
    store::{StoreContext, StoreContextMut},
    Engine,
};

/// Extern host reference type
#[derive(Clone, Debug)]
pub struct ExternRef {
    /// The inner extern ref object
    object: Py<PyAny>,
}

impl WasmExternRef<Engine> for ExternRef {
    fn new<T: 'static + Send + Sync>(
        mut ctx: impl AsContextMut<Engine>,
        object: Option<T>,
    ) -> Self {
        Python::with_gil(|py| {
            // see https://github.com/DouglasDwyer/wasm_runtime_layer/issues/5
            let Some(object) = object else {
                anyhow::bail!("use `None` instead of `Some(ExternRef(None))`");
            };
            let object = Box::new(object);

            let mut store: StoreContextMut<_> = ctx.as_context_mut();

            let object = Py::new(
                py,
                PyExternRef {
                    object: store.register_externref(object),
                },
            )?;
            let object = py_to_js_proxy(py, object)?;

            Ok(Self { object })
        })
        .unwrap()
    }

    fn downcast<'a, T: 'static, S: 'a>(
        &self,
        ctx: StoreContext<'a, S>,
    ) -> anyhow::Result<Option<&'a T>> {
        Python::with_gil(|py| {
            let Ok(object): Result<Py<PyExternRef>, _> = self.object.extract(py) else {
                anyhow::bail!("extern ref is from a different source");
            };
            let object = object.as_ref(py).try_borrow().map_err(PyErr::from)?;

            let object: &'a AnyExternRef = ctx.get_externref(object.object)?;

            let Some(object) = object.downcast_ref() else {
                anyhow::bail!("incorrect extern ref type");
            };

            Ok(Some(object))
        })
    }
}

impl ToPy for ExternRef {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        self.object.clone_ref(py)
    }
}

impl ExternRef {
    /// Creates a new extern ref from a Python value
    pub(crate) fn from_exported_externref(object: Py<PyAny>) -> Self {
        Self { object }
    }
}

pub type AnyExternRef = dyn 'static + Any + Send + Sync;

#[pyclass(frozen)]
struct PyExternRef {
    /// Reference to the extern ref data which is stored in the store
    ///
    /// If the ExternRef API is changed to decouple the downcast lifetime
    /// from the store, this could just store the object itself.
    object: Id<Box<AnyExternRef>>,
}

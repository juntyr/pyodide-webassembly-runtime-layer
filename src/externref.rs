use std::{any::Any, sync::Arc};

use pyo3::prelude::*;
use wasm_runtime_layer::backend::{AsContextMut, WasmExternRef};

use crate::{
    conversion::{py_to_js_proxy, ToPy},
    store::StoreContext,
    Engine,
};

/// Extern host reference type.
#[derive(Clone, Debug)]
pub struct ExternRef {
    /// The inner extern ref object, for host access, optional
    host: Option<Arc<AnyExternRef>>,
    /// The inner extern ref object, for guest access, opaque
    guest: Py<PyAny>,
}

impl WasmExternRef<Engine> for ExternRef {
    fn new<T: 'static + Send + Sync>(_ctx: impl AsContextMut<Engine>, object: T) -> Self {
        Python::with_gil(|py| -> Result<Self, PyErr> {
            let object: Arc<AnyExternRef> = Arc::new(object);

            let guest = Bound::new(
                py,
                PyExternRef {
                    object: Arc::clone(&object),
                },
            )?;
            let guest = py_to_js_proxy(guest)?;

            Ok(Self {
                host: Some(object),
                guest: guest.unbind(),
            })
        })
        .unwrap()
    }

    fn downcast<'a, 's: 'a, T: 'static, S: 's>(
        &'a self,
        _ctx: StoreContext<'s, S>,
    ) -> anyhow::Result<&'a T> {
        // Check if we have a host-accessible non-opaque reference to the data
        let Some(object) = self.host.as_ref() else {
            anyhow::bail!("extern ref is from a different source");
        };

        let Some(object) = object.downcast_ref() else {
            anyhow::bail!("incorrect extern ref type");
        };

        Ok(object)
    }
}

impl ToPy for ExternRef {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        self.guest.clone_ref(py)
    }
}

impl ExternRef {
    /// Creates a new extern ref from a Python value
    pub(crate) fn from_exported_externref(object: Bound<PyAny>) -> Self {
        // Check if this ExternRef comes from this source,
        //  if not, return an opaque ExternRef
        let Ok(host): Result<Bound<PyExternRef>, _> = object.extract() else {
            return Self {
                host: None,
                guest: object.unbind(),
            };
        };

        let host = Arc::clone(&host.get().object);

        Self {
            host: Some(host),
            guest: object.unbind(),
        }
    }
}

type AnyExternRef = dyn 'static + Any + Send + Sync;

#[pyclass(frozen)]
struct PyExternRef {
    /// The inner extern ref data
    object: Arc<AnyExternRef>,
}

use std::sync::OnceLock;

use pyo3::{intern, prelude::*};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmTable},
    TableType, ValueType,
};

use crate::{
    conversion::{create_js_object, instanceof, ToPy, ValueExt, ValueTypeExt},
    Engine,
};

#[derive(Clone, Debug)]
/// A WASM table.
///
/// This type wraps a [`WebAssembly.Table`] from the JavaScript API.
///
/// [`WebAssembly.Table`]: https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Table
pub struct Table {
    /// Table reference
    table: Py<PyAny>,
    /// The table signature
    ty: TableType,
}

impl WasmTable<Engine> for Table {
    fn new(
        _ctx: impl AsContextMut<Engine>,
        ty: TableType,
        init: Value<Engine>,
    ) -> anyhow::Result<Self> {
        Python::with_gil(|py| -> anyhow::Result<Self> {
            #[cfg(feature = "tracing")]
            tracing::debug!(?ty, ?init, "Table::new");

            let desc = create_js_object(py)?;
            desc.setattr(intern!(py, "element"), ty.element().as_js_descriptor())?;
            desc.setattr(intern!(py, "initial"), ty.minimum())?;
            if let Some(max) = ty.maximum() {
                desc.setattr(intern!(py, "maximum"), max)?;
            }

            let init = init.to_py(py);

            let table = web_assembly_table(py).call_method1(intern!(py, "new"), (desc, init))?;

            Ok(Self {
                table: table.unbind(),
                ty,
            })
        })
    }

    /// Returns the type and limits of the table.
    fn ty(&self, _ctx: impl AsContext<Engine>) -> TableType {
        self.ty
    }

    /// Returns the current size of the table.
    fn size(&self, _ctx: impl AsContext<Engine>) -> u32 {
        Python::with_gil(|py| -> Result<u32, PyErr> {
            let table = self.table.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(table = %table, ?self.ty, "Table::size");

            table.getattr(intern!(py, "length"))?.extract()
        })
        .unwrap()
    }

    /// Grows the table by the given amount of elements.
    fn grow(
        &self,
        _ctx: impl AsContextMut<Engine>,
        delta: u32,
        init: Value<Engine>,
    ) -> anyhow::Result<u32> {
        Python::with_gil(|py| {
            let table = self.table.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(table = %table, ?self.ty, delta, ?init, "Table::grow");

            let init = init.to_py(py);

            let old_len = table
                .call_method1(intern!(py, "grow"), (delta, init))?
                .extract()?;

            Ok(old_len)
        })
    }

    /// Returns the table element value at `index`.
    fn get(&self, _ctx: impl AsContextMut<Engine>, index: u32) -> Option<Value<Engine>> {
        Python::with_gil(|py| {
            let table = self.table.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(table = %table, ?self.ty, index, "Table::get");

            let value = table.call_method1(intern!(py, "get"), (index,)).ok()?;

            Some(Value::from_py_typed(value, self.ty.element()).unwrap())
        })
    }

    /// Sets the value of this table at `index`.
    fn set(
        &self,
        _ctx: impl AsContextMut<Engine>,
        index: u32,
        value: Value<Engine>,
    ) -> anyhow::Result<()> {
        Python::with_gil(|py| {
            let table = self.table.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(table = %table, ?self.ty, index, ?value, "Table::set");

            let value = value.to_py(py);

            table.call_method1(intern!(py, "set"), (index, value))?;

            Ok(())
        })
    }
}

impl ToPy for Table {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(table = %self.table, ?self.ty, "Table::to_py");

        self.table.clone_ref(py)
    }
}

impl Table {
    /// Creates a new table from a Python value
    pub(crate) fn from_exported_table(table: Bound<PyAny>, ty: TableType) -> anyhow::Result<Self> {
        if !instanceof(&table, web_assembly_table(table.py()))? {
            anyhow::bail!("expected WebAssembly.Table but found {}", table);
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(table = %table, ?ty, "Table::from_exported_table");

        let table_length: u32 = table.getattr(intern!(table.py(), "length"))?.extract()?;

        assert!(table_length >= ty.minimum());
        assert_eq!(ty.element(), ValueType::FuncRef);

        Ok(Self {
            table: table.unbind(),
            ty,
        })
    }
}

fn web_assembly_table(py: Python) -> &Bound<PyAny> {
    static WEB_ASSEMBLY_TABLE: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    WEB_ASSEMBLY_TABLE
        .get_or_init(|| {
            py.import_bound(intern!(py, "js"))
                .unwrap()
                .getattr(intern!(py, "WebAssembly"))
                .unwrap()
                .getattr(intern!(py, "Table"))
                .unwrap()
                .into_py(py)
        })
        .bind(py)
}

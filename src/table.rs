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
/// A WebAssembly table
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
            desc.setattr(py, intern!(py, "element"), ty.element().as_js_descriptor())?;
            desc.setattr(py, intern!(py, "initial"), ty.minimum())?;
            if let Some(max) = ty.maximum() {
                desc.setattr(py, intern!(py, "maximum"), max)?;
            }

            let init = init.to_py(py);

            let table =
                web_assembly_table(py).call_method1(py, intern!(py, "new"), (desc, init))?;

            Ok(Self { table, ty })
        })
    }

    /// Returns the type and limits of the table.
    fn ty(&self, _ctx: impl AsContext<Engine>) -> TableType {
        self.ty
    }

    /// Returns the current size of the table.
    fn size(&self, _ctx: impl AsContext<Engine>) -> u32 {
        Python::with_gil(|py| -> Result<u32, PyErr> {
            #[cfg(feature = "tracing")]
            tracing::debug!(table = %self.table.as_ref(py), ?self.ty, "Table::size");

            self.table.getattr(py, intern!(py, "length"))?.extract(py)
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
            #[cfg(feature = "tracing")]
            tracing::debug!(table = %self.table.as_ref(py), ?self.ty, delta, ?init, "Table::grow");

            let init = init.to_py(py);

            let old_len = self
                .table
                .call_method1(py, intern!(py, "grow"), (delta, init))?
                .extract(py)?;

            Ok(old_len)
        })
    }

    /// Returns the table element value at `index`.
    fn get(&self, _ctx: impl AsContextMut<Engine>, index: u32) -> Option<Value<Engine>> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(table = %self.table.as_ref(py), ?self.ty, index, "Table::get");

            let value = self
                .table
                .call_method1(py, intern!(py, "get"), (index,))
                .ok()?;

            Some(Value::from_py_typed(py, value, self.ty.element()).unwrap())
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
            #[cfg(feature = "tracing")]
            tracing::debug!(table = %self.table.as_ref(py), ?self.ty, index, ?value, "Table::set");

            let value = value.to_py(py);

            self.table
                .call_method1(py, intern!(py, "set"), (index, value))?;

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
    pub(crate) fn from_exported_table(
        py: Python,
        table: Py<PyAny>,
        ty: TableType,
    ) -> anyhow::Result<Self> {
        if !instanceof(py, &table, web_assembly_table(py))? {
            anyhow::bail!("expected WebAssembly.Table but found {}", table.as_ref(py));
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(table = %table.as_ref(py), ?ty, "Table::from_exported_table");

        let table_length: u32 = table.getattr(py, intern!(py, "length"))?.extract(py)?;

        assert!(table_length >= ty.minimum());
        assert_eq!(ty.element(), ValueType::FuncRef);

        Ok(Self { table, ty })
    }
}

fn web_assembly_table(py: Python) -> &'static Py<PyAny> {
    static WEB_ASSEMBLY_TABLE: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    WEB_ASSEMBLY_TABLE.get_or_init(|| {
        py.import(intern!(py, "js"))
            .unwrap()
            .getattr(intern!(py, "WebAssembly"))
            .unwrap()
            .getattr(intern!(py, "Table"))
            .unwrap()
            .into_py(py)
    })
}

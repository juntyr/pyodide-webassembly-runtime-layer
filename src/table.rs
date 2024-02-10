use pyo3::{intern, prelude::*, types::PyDict};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmTable},
    TableType, ValueType,
};

use crate::{
    conversion::{instanceof, py_dict_to_js_object, ToPy, ValueExt, ValueTypeExt},
    Engine,
};

#[derive(Debug, Clone)]
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
            let _span = tracing::debug_span!("Table::new", ?ty, ?init).entered();

            let desc = PyDict::new(py);
            desc.set_item(intern!(py, "element"), ty.element().as_js_descriptor())?;
            desc.set_item(intern!(py, "initial"), ty.minimum())?;
            if let Some(max) = ty.maximum() {
                desc.set_item(intern!(py, "maximum"), max)?;
            }
            let desc = py_dict_to_js_object(py, desc)?;

            // init is passed to WebAssembly table, so it must be turned into JS
            let init = init.to_py_js(py)?;

            let table = web_assembly_table(py)?
                .getattr(intern!(py, "new"))?
                .call1((desc, init))?;

            Ok(Self {
                ty,
                table: table.into_py(py),
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
            let table = self.table.as_ref(py);
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
            // init is passed to WebAssembly table, so it must be turned into JS
            let init = init.to_py_js(py)?;

            let table = self.table.as_ref(py);

            let old_len = table
                .call_method1(intern!(py, "grow"), (delta, init))?
                .extract()?;

            Ok(old_len)
        })
    }

    /// Returns the table element value at `index`.
    fn get(&self, _ctx: impl AsContextMut<Engine>, index: u32) -> Option<Value<Engine>> {
        Python::with_gil(|py| {
            let table = self.table.as_ref(py);

            let value = table.call_method1(intern!(py, "get"), (index,)).ok()?;

            Some(Value::from_py_typed(value, &self.ty.element()).unwrap())
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
            // value is passed to WebAssembly global, so it must be turned into JS
            let value = value.to_py_js(py)?;

            let table = self.table.as_ref(py);

            table.call_method1(intern!(py, "set"), (index, value))?;

            Ok(())
        })
    }
}

impl ToPy for Table {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        self.table.clone_ref(py)
    }
}

impl Table {
    /// Creates a new table from a Python value
    pub(crate) fn from_exported_table(value: &PyAny, ty: TableType) -> anyhow::Result<Self> {
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("Table::from_py", value).entered();

        let py = value.py();

        if !instanceof(py, value, web_assembly_table(py)?)? {
            anyhow::bail!("expected WebAssembly.Table but found {value:?}");
        }

        let table_length: u32 = value.getattr(intern!(py, "length"))?.extract()?;

        assert!(table_length >= ty.minimum());
        assert_eq!(ty.element(), ValueType::FuncRef);

        Ok(Self {
            ty,
            table: value.into_py(py),
        })
    }
}

fn web_assembly_table(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Table"))
}

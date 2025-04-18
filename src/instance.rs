use std::{collections::BTreeMap, sync::Arc};

use fxhash::FxHashMap;
use pyo3::{intern, prelude::*, sync::GILOnceCell};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Export, Extern, Imports, WasmInstance, WasmModule},
    ExportType, ExternType,
};

use crate::{
    conversion::{create_js_object, ToPy},
    Engine, Func, Global, Memory, Module, Table,
};

/// An instantiated instance of a WASM [`Module`].
///
/// This type wraps a [`WebAssembly.Instance`] from the JavaScript API.
///
/// [`WebAssembly.Instance`]: https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Instance
#[derive(Debug)]
pub struct Instance {
    /// The inner instance
    instance: Py<PyAny>,
    /// The exports of the instance
    exports: Arc<FxHashMap<String, Extern<Engine>>>,
}

impl Clone for Instance {
    fn clone(&self) -> Self {
        Python::with_gil(|py| Self {
            instance: self.instance.clone_ref(py),
            exports: self.exports.clone(),
        })
    }
}

impl WasmInstance<Engine> for Instance {
    fn new(
        _store: impl AsContextMut<Engine>,
        module: &Module,
        imports: &Imports<Engine>,
    ) -> anyhow::Result<Self> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("Instance::new").entered();

            let imports_object = create_imports_object(py, imports)?;

            let instance =
                web_assembly_instance_new(py)?.call1((module.module(py), imports_object))?;

            let exports = instance.getattr(intern!(py, "exports"))?;
            let exports = process_exports(&exports, module)?;

            Ok(Self {
                instance: instance.unbind(),
                exports: Arc::new(exports),
            })
        })
    }

    fn exports(&self, _store: impl AsContext<Engine>) -> Box<dyn Iterator<Item = Export<Engine>>> {
        Box::new(
            self.exports
                .iter()
                .map(|(name, value)| Export {
                    name: name.into(),
                    value: value.clone(),
                })
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }

    fn get_export(&self, _store: impl AsContext<Engine>, name: &str) -> Option<Extern<Engine>> {
        self.exports.get(name).cloned()
    }
}

/// Creates the js import map
fn create_imports_object<'py>(
    py: Python<'py>,
    imports: &Imports<Engine>,
) -> Result<Bound<'py, PyAny>, PyErr> {
    #[cfg(feature = "tracing")]
    let _span = tracing::debug_span!("process_imports").entered();

    let imports = imports
        .iter()
        .map(|(module, name, import)| -> Result<_, PyErr> {
            #[cfg(feature = "tracing")]
            tracing::trace!(?module, ?name, ?import, "import");

            let import = import.to_py(py);

            #[cfg(feature = "tracing")]
            tracing::trace!(module, name, "export");

            Ok((module, (name, import)))
        })
        .try_fold(
            BTreeMap::<&str, Vec<_>>::new(),
            |mut acc, elem| -> Result<_, PyErr> {
                let (module, value) = elem?;
                acc.entry(module).or_default().push(value);
                Ok(acc)
            },
        )?
        .into_iter()
        .try_fold(
            create_js_object(py)?,
            |acc, (module, imports)| -> Result<_, PyErr> {
                let obj = create_js_object(py)?;
                for (name, import) in imports {
                    obj.setattr(name, import)?;
                }
                acc.setattr(module, obj)?;
                Ok(acc)
            },
        )?;

    Ok(imports)
}

/// Processes a wasm module's exports into a hashmap
fn process_exports(
    exports: &Bound<PyAny>,
    module: &Module,
) -> anyhow::Result<FxHashMap<String, Extern<Engine>>> {
    #[cfg(feature = "tracing")]
    let _span = tracing::debug_span!("process_exports").entered();

    module
        .exports()
        .map(|ExportType { name, ty }| {
            let export = match ty {
                ExternType::Func(signature) => Extern::Func(Func::from_exported_function(
                    exports.getattr(name)?,
                    signature,
                )?),
                ExternType::Global(signature) => Extern::Global(Global::from_exported_global(
                    exports.getattr(name)?,
                    signature,
                )?),
                ExternType::Memory(ty) => {
                    Extern::Memory(Memory::from_exported_memory(exports.getattr(name)?, ty)?)
                },
                ExternType::Table(ty) => {
                    Extern::Table(Table::from_exported_table(exports.getattr(name)?, ty)?)
                },
            };

            Ok((String::from(name), export))
        })
        .collect()
}

fn web_assembly_instance_new(py: Python) -> Result<&Bound<PyAny>, PyErr> {
    static WEB_ASSEMBLY_INSTANCE: GILOnceCell<Py<PyAny>> = GILOnceCell::new();
    WEB_ASSEMBLY_INSTANCE.import(py, "js.WebAssembly.Instance", "new")
}

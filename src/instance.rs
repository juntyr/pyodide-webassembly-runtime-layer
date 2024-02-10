use std::collections::BTreeMap;

use fxhash::FxHashMap;
use pyo3::{intern, prelude::*, types::IntoPyDict};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Export, Extern, Imports, WasmInstance, WasmModule},
    ExternType,
};

use crate::{
    conversion::{py_dict_to_js_object, ToPy},
    Engine, Func, Global, Memory, Module, Table,
};

/// A WebAssembly Instance
#[derive(Debug, Clone)]
pub struct Instance {
    /// The inner instance
    _instance: Py<PyAny>,
    /// The exports of the instance
    exports: FxHashMap<String, Extern<Engine>>,
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

            let instance = web_assembly_instance(py)?
                .getattr(intern!(py, "new"))?
                .call1((module.module(py), imports_object))?;

            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("get_exports").entered();

            let exports = instance.getattr(intern!(py, "exports"))?;
            let exports = process_exports(exports, module)?;

            Ok(Self {
                _instance: instance.into_py(py),
                exports,
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
) -> Result<&'py PyAny, PyErr> {
    #[cfg(feature = "tracing")]
    let _span = tracing::debug_span!("process_imports").entered();

    let imports = imports
        .into_iter()
        .map(|((module, name), import)| -> Result<_, PyErr> {
            #[cfg(feature = "tracing")]
            tracing::trace!(?module, ?name, ?import, "import");
            // import is passed to WebAssembly instantiation, so it must be turned into JS
            let import = import.to_py_js(py)?;

            #[cfg(feature = "tracing")]
            tracing::trace!(module, name, "export");

            Ok((module, (name, import)))
        })
        .try_fold(
            BTreeMap::<String, Vec<_>>::new(),
            |mut acc, elem| -> Result<_, PyErr> {
                let (module, value) = elem?;
                acc.entry(module).or_default().push(value);
                Ok(acc)
            },
        )?
        .into_iter()
        .map(|(module, imports)| (module, imports.into_py_dict(py)))
        .into_py_dict(py);

    py_dict_to_js_object(py, imports)
}

/// Processes a wasm module's exports into a hashmap
fn process_exports(
    exports: &PyAny,
    module: &Module,
) -> anyhow::Result<FxHashMap<String, Extern<Engine>>> {
    let py = exports.py();

    #[cfg(feature = "tracing")]
    let _span = tracing::debug_span!("process_exports", ?exports).entered();

    exports
        .call_method0(intern!(py, "object_entries"))?
        .iter()?
        .map(|entry| {
            let (name, value): (String, &PyAny) = entry?.extract()?;

            #[cfg(feature = "tracing")]
            let _span = tracing::trace_span!("process_export", ?name, ?value).entered();

            let signature = module.get_export(&name).expect("export signature");

            let export = match signature {
                ExternType::Func(signature) => {
                    Extern::Func(Func::from_exported_function(value, signature)?)
                }
                ExternType::Global(signature) => {
                    Extern::Global(Global::from_exported_global(value, signature)?)
                }
                ExternType::Memory(ty) => Extern::Memory(Memory::from_exported_memory(value, ty)?),
                ExternType::Table(ty) => Extern::Table(Table::from_exported_table(value, ty)?),
            };

            Ok((name, export))
        })
        .collect()
}

fn web_assembly_instance(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Instance"))
}

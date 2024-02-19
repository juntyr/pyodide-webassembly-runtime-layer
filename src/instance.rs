use std::{collections::BTreeMap, sync::OnceLock};

use fxhash::FxHashMap;
use pyo3::{intern, prelude::*};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Export, Extern, Imports, WasmInstance, WasmModule},
    ExportType, ExternType,
};

use crate::{
    conversion::{create_js_object, ToPy},
    Engine, Func, Global, Memory, Module, Table,
};

/// A WebAssembly Instance
#[derive(Clone, Debug)]
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

            let instance = web_assembly_instance(py)
                .getattr(py, intern!(py, "new"))?
                .call1(py, (module.module(py), imports_object))?;

            let exports = instance.getattr(py, intern!(py, "exports"))?;
            let exports = process_exports(py, &exports, module)?;

            Ok(Self {
                _instance: instance,
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
fn create_imports_object(py: Python, imports: &Imports<Engine>) -> Result<Py<PyAny>, PyErr> {
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
                    obj.setattr(py, name, import)?;
                }
                acc.setattr(py, module, obj)?;
                Ok(acc)
            },
        )?;

    Ok(imports)
}

/// Processes a wasm module's exports into a hashmap
fn process_exports(
    py: Python,
    exports: &Py<PyAny>,
    module: &Module,
) -> anyhow::Result<FxHashMap<String, Extern<Engine>>> {
    #[cfg(feature = "tracing")]
    let _span = tracing::debug_span!("process_exports").entered();

    module
        .exports()
        .map(|ExportType { name, ty }| {
            let export = match ty {
                ExternType::Func(signature) => Extern::Func(Func::from_exported_function(
                    py,
                    exports.getattr(py, name)?,
                    signature,
                )?),
                ExternType::Global(signature) => Extern::Global(Global::from_exported_global(
                    py,
                    exports.getattr(py, name)?,
                    signature,
                )?),
                ExternType::Memory(ty) => Extern::Memory(Memory::from_exported_memory(
                    py,
                    exports.getattr(py, name)?,
                    ty,
                )?),
                ExternType::Table(ty) => Extern::Table(Table::from_exported_table(
                    py,
                    exports.getattr(py, name)?,
                    ty,
                )?),
            };

            Ok((String::from(name), export))
        })
        .collect()
}

fn web_assembly_instance(py: Python) -> &'static Py<PyAny> {
    static WEB_ASSEMBLY_INSTANCE: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    WEB_ASSEMBLY_INSTANCE.get_or_init(|| {
        py.import(intern!(py, "js"))
            .unwrap()
            .getattr(intern!(py, "WebAssembly"))
            .unwrap()
            .getattr(intern!(py, "Instance"))
            .unwrap()
            .into_py(py)
    })
}

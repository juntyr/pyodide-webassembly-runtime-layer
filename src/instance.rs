use std::collections::BTreeMap;
use std::sync::Arc;

use fxhash::FxHashMap;
use pyo3::{
    intern,
    prelude::*,
    // types::{IntoPyDict, PyTuple},
    // types::PyTuple,
    // PyTypeInfo,
};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Export, Extern, Imports, WasmInstance, WasmModule},
    ExternType, ExportType,
};

use crate::{
    // conversion::{py_dict_to_js_object, ToPy},
    conversion::ToPy,
    Engine, Func, Global, Memory, Module, Table,
};

/// A WebAssembly Instance
#[derive(Debug)]
pub struct Instance {
    /// The inner instance
    instance: Py<PyAny>,
    /// The exports of the instance
    exports: FxHashMap<String, Extern<Engine>>,
}

impl Drop for Instance {
    fn drop(&mut self) {
        Python::with_gil(|py| {
            let instance = std::mem::replace(&mut self.instance, py.None());

            #[cfg(feature = "tracing")]
            tracing::debug!(refcnt = instance.get_refcnt(py), "Instance::drop");

            // Safety: we hold the GIL and own global
            unsafe { pyo3::ffi::Py_DECREF(instance.into_ptr()) };
        })
    }
}

impl Clone for Instance {
    fn clone(&self) -> Self {
        // if self.exports.iter().any(|(name, value)| name == "2") {
        //     println!("INSTANCE CLONE {self:?}");
        // }

        Python::with_gil(|py| {
            Self {
                instance: self.instance.clone_ref(py),
                exports: self.exports.clone(),
            }
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

            let instance = web_assembly_instance(py)?
                .getattr(intern!(py, "new"))?
                .call1((module.module(py), imports_object))?
                .into_py(py);

            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("get_exports").entered();

            // let exports = instance.getattr(py, intern!(py, "exports"))?;
            // let instance = Arc::new(instance);
            let exports = process_exports(/*&instance,*/ py, &instance, module)?;

            Ok(Self {
                instance,//: Arc::new(instance.into_py(py)),
                exports,
            })
        })
    }

    fn exports(&self, _store: impl AsContext<Engine>) -> Box<dyn Iterator<Item = Export<Engine>>> {
        // if self.exports.iter().any(|(name, value)| name == "2") {
        //     println!("ITER_EXPORTS {:?}", self.exports);
        // }
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
        // if name == "2" {
        //     println!("GET_EXPORT {:?}", self.exports.get(name));
        // }
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

    // println!("IMPORTS");

    let imports = imports
        .iter()
        .map(|(module, name, import)| -> Result<_, PyErr> {
            #[cfg(feature = "tracing")]
            tracing::trace!(?module, ?name, ?import, "import");

            // println!("IMPORT {module} {name} {import:?}");

            // import is passed to WebAssembly instantiation, so it must be turned into JS
            let import = import.to_py_js(py)?;

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
        // .map(|(module, imports)| (module, imports.into_py_dict(py)))
        .try_fold(
            py.import(intern!(py, "js"))?
                .getattr(intern!(py, "Object"))?
                .call_method0(intern!(py, "new"))?,
            |acc, (module, imports)| -> Result<_, PyErr> {
                let obj = py.import(intern!(py, "js"))?
                    .getattr(intern!(py, "Object"))?
                    .call_method0(intern!(py, "new"))?;
                for (name, import) in imports {
                    obj.setattr(name, import)?;
                }
                acc.setattr(module, obj)?;
                Ok(acc)
            },
        )?;
        // .into_py_dict(py);

    // py_dict_to_js_object(py, imports)
    Ok(imports)
}

/// Processes a wasm module's exports into a hashmap
fn process_exports(
    py: Python,
    // instance: &Arc<Py<PyAny>>,
    // exports: Py<PyAny>,
    instance: &Py<PyAny>,
    module: &Module,
) -> anyhow::Result<FxHashMap<String, Extern<Engine>>> {
    // let py = exports.py();

    // #[cfg(feature = "tracing")]
    // let _span = tracing::debug_span!("process_exports", %exports).entered();

    // println!("EXPORTS");

    module.exports().map(|ExportType { name, ty }| {
        let export = match ty {
            ExternType::Func(signature) => {
                Extern::Func(Func::from_exported_function(py, /*exports.getattr(py, name)?*/get_instance_export(py, instance, name)?, signature)?)
            }
            ExternType::Global(signature) => {
                Extern::Global(Global::from_exported_global(py, get_instance_export(py, instance, name)?, signature)?)
            }
            ExternType::Memory(ty) => Extern::Memory(Memory::from_exported_memory(py, get_instance_export(py, instance, name)?, ty)?),
            ExternType::Table(ty) => Extern::Table(Table::from_exported_table(py, get_instance_export(py, instance, name)?, ty)?),
        };

        Ok((String::from(name), export))
    }).collect()

    // exports
    //     .call_method0(intern!(py, "object_keys"))?
    //     .iter()?
    //     .map(|name| {
    //         let name: String = name?.extract()?;
    //         // let entry = PyTuple::type_object(py).call1((entry?,))?;
    //         // let (name, value): (String, &PyAny) = entry.extract()?;
    //         // let value = exports.getattr(name.as_str())?;

    //         // #[cfg(feature = "tracing")]
    //         // tracing::trace!(?name, %value, "process_export");

    //         let signature = module.get_export(&name).expect("export signature");

    //         // println!("EXPORT {name} {signature:?}");

    //         let export = match signature {
    //             ExternType::Func(signature) => {
    //                 Extern::Func(Func::from_exported_function(instance, &name, signature)?)
    //             }
    //             ExternType::Global(signature) => {
    //                 Extern::Global(Global::from_exported_global(exports.getattr(name.as_str())?, signature)?)
    //             }
    //             ExternType::Memory(ty) => Extern::Memory(Memory::from_exported_memory(exports.getattr(name.as_str())?, ty)?),
    //             ExternType::Table(ty) => Extern::Table(Table::from_exported_table(exports.getattr(name.as_str())?, ty)?),
    //         };

    //         Ok((name, export))
    //     })
    //     .collect()
}

fn get_instance_export(py: Python, instance: &Py<PyAny>, name: &str) -> Result<Py<PyAny>, PyErr> {
    fn get_export(py: Python) -> &'static Py<PyAny> {
        static GET_EXPORT: std::sync::OnceLock<Py<PyAny>> = std::sync::OnceLock::new();
        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        GET_EXPORT.get_or_init(|| {
            py
                .import(intern!(py, "pyodide")).unwrap()
                .getattr(intern!(py, "code")).unwrap()
                .getattr(intern!(py, "run_js")).unwrap()
                .call1((
                    "function get_export(instance, name){ let exp = instance.exports[name]; /*console.warn(\"export\", name, exp);*/ return exp; } get_export",
                )).unwrap()
                .into_py(py)
        })
    }

    let export = get_export(py).call1(py, (instance, name))?;

    // println!("EXPORT {name} jsid={:?} refcnt={}", export.as_ref(py).getattr(intern!(py, "js_id")), export.get_refcnt(py));

    Ok(export)
}

fn web_assembly_instance(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Instance"))
}

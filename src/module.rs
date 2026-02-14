use std::sync::Arc;

use fxhash::FxHashMap;
use pyo3::{prelude::*, sync::PyOnceLock};
use wasm_runtime_layer::{
    backend::WasmModule, ExportType, ExternType, FuncType, GlobalType, ImportType, MemoryType,
    TableType, ValueType,
};

use crate::{
    conversion::js_uint8_array_new, features::UnsupportedWasmFeatureExtensionError, Engine,
};

#[derive(Debug)]
/// A WASM module.
///
/// This type wraps a [`WebAssembly.Module`] from the JavaScript API.
///
/// [`WebAssembly.Module`]: https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Module
pub struct Module {
    /// The inner module
    module: Py<PyAny>,
    /// The parsed module, containing import and export signatures
    parsed: Arc<ParsedModule>,
}

impl Clone for Module {
    fn clone(&self) -> Self {
        Python::attach(|py| Self {
            module: self.module.clone_ref(py),
            parsed: self.parsed.clone(),
        })
    }
}

impl WasmModule<Engine> for Module {
    fn new(_engine: &Engine, bytes: &[u8]) -> anyhow::Result<Self> {
        Python::attach(|py| {
            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("Module::new").entered();

            let parsed = ParsedModule::parse(bytes)?;

            let buffer = js_uint8_array_new(py)?.call1((bytes,))?;

            let module = match web_assembly_module_new(py)?.call1((buffer,)) {
                Ok(module) => module,
                // check if the error comes from missing feature support
                // - if so, report the more informative unsupported feature error instead
                // - if not, bubble up the error that made module instantiation fail
                Err(err) => match Python::attach(|py| {
                    UnsupportedWasmFeatureExtensionError::check_support(py, bytes)
                })? {
                    Ok(()) => anyhow::bail!(err),
                    Err(unsupported) => anyhow::bail!(unsupported),
                },
            };

            let parsed = Arc::new(parsed);

            Ok(Self {
                module: module.unbind(),
                parsed,
            })
        })
    }

    fn exports(&self) -> Box<dyn '_ + Iterator<Item = ExportType<'_>>> {
        Box::new(self.parsed.exports.iter().map(|(name, ty)| ExportType {
            name: name.as_str(),
            ty: ty.clone(),
        }))
    }

    fn get_export(&self, name: &str) -> Option<ExternType> {
        self.parsed.exports.get(name).cloned()
    }

    fn imports(&self) -> Box<dyn '_ + Iterator<Item = ImportType<'_>>> {
        Box::new(
            self.parsed
                .imports
                .iter()
                .map(|((module, name), kind)| ImportType {
                    module,
                    name,
                    ty: kind.clone(),
                }),
        )
    }
}

impl Module {
    pub(crate) fn module(&self, py: Python) -> Py<PyAny> {
        self.module.clone_ref(py)
    }
}

#[derive(Debug)]
/// A parsed core module with imports and exports
struct ParsedModule {
    /// Import signatures
    imports: FxHashMap<(String, String), ExternType>,
    /// Export signatures
    exports: FxHashMap<String, ExternType>,
}

impl ParsedModule {
    #[allow(clippy::too_many_lines)]
    /// Parses a module from bytes and extracts import and export signatures
    fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        let parser = wasmparser::Parser::new(0);

        let mut imports = FxHashMap::default();
        let mut exports = FxHashMap::default();

        let mut types = Vec::new();

        let mut functions = Vec::new();
        let mut tables = Vec::new();
        let mut memories = Vec::new();
        let mut globals = Vec::new();

        parser.parse_all(bytes).try_for_each(|payload| {
            match payload? {
                wasmparser::Payload::TypeSection(section) => {
                    for ty in section.into_iter_err_on_gc_types() {
                        types.push(Type::Func(Partial::Lazy(ty?)));
                    }
                },
                wasmparser::Payload::ImportSection(section) => {
                    for import in section.into_imports() {
                        let import = import?;
                        let ty = match import.ty {
                            wasmparser::TypeRef::Func(index)
                            | wasmparser::TypeRef::FuncExact(index) => {
                                let Type::Func(ty) = &mut types[index as usize];
                                let ty = ty.force(|ty| FuncType::from_parsed(ty))?;
                                let func = ty.clone().with_name(import.name);
                                functions.push(Partial::Eager(func.clone()));
                                ExternType::Func(func)
                            },
                            wasmparser::TypeRef::Table(ty) => {
                                let table = TableType::from_parsed(&ty)?;
                                tables.push(Partial::Eager(table));
                                ExternType::Table(table)
                            },
                            wasmparser::TypeRef::Memory(ty) => {
                                let memory = MemoryType::from_parsed(&ty)?;
                                memories.push(Partial::Eager(memory));
                                ExternType::Memory(memory)
                            },
                            wasmparser::TypeRef::Global(ty) => {
                                let global = GlobalType::from_parsed(&ty)?;
                                globals.push(Partial::Eager(global));
                                ExternType::Global(global)
                            },
                            wasmparser::TypeRef::Tag(_) => {
                                anyhow::bail!(
                                    "tag imports are not yet supported in the wasm_runtime_layer"
                                )
                            },
                        };

                        imports.insert((import.module.to_string(), import.name.to_string()), ty);
                    }
                },
                wasmparser::Payload::FunctionSection(section) => {
                    for type_index in section {
                        let type_index = type_index?;
                        let Type::Func(ty) = &types[type_index as usize];
                        functions.push(ty.clone());
                    }
                },
                wasmparser::Payload::TableSection(section) => {
                    for table in section {
                        tables.push(Partial::Lazy(table?.ty));
                    }
                },
                wasmparser::Payload::MemorySection(section) => {
                    for memory in section {
                        memories.push(Partial::Lazy(memory?));
                    }
                },
                wasmparser::Payload::GlobalSection(section) => {
                    for global in section {
                        globals.push(Partial::Lazy(global?.ty));
                    }
                },
                wasmparser::Payload::ExportSection(section) => {
                    for export in section {
                        let export = export?;
                        let index = export.index as usize;
                        let ty = match export.kind {
                            wasmparser::ExternalKind::Func
                            | wasmparser::ExternalKind::FuncExact => {
                                let ty = functions[index].force(|ty| FuncType::from_parsed(ty))?;
                                let func = ty.clone().with_name(export.name);
                                ExternType::Func(func)
                            },
                            wasmparser::ExternalKind::Table => {
                                let table = tables[index].force(|ty| TableType::from_parsed(ty))?;
                                ExternType::Table(*table)
                            },
                            wasmparser::ExternalKind::Memory => {
                                let memory =
                                    memories[index].force(|ty| MemoryType::from_parsed(ty))?;
                                ExternType::Memory(*memory)
                            },
                            wasmparser::ExternalKind::Global => {
                                let global =
                                    globals[index].force(|ty| GlobalType::from_parsed(ty))?;
                                ExternType::Global(*global)
                            },
                            wasmparser::ExternalKind::Tag => {
                                anyhow::bail!(
                                    "tag exports are not yet supported in the wasm_runtime_layer"
                                )
                            },
                        };

                        exports.insert(export.name.to_string(), ty);
                    }
                },
                _ => (),
            }

            anyhow::Ok(())
        })?;

        Ok(Self { imports, exports })
    }
}

enum Type<T> {
    Func(T),
}

#[derive(Clone)]
enum Partial<L, E> {
    Lazy(L),
    Eager(E),
}

impl<L, E> Partial<L, E> {
    fn force(&mut self, eval: impl FnOnce(&mut L) -> anyhow::Result<E>) -> anyhow::Result<&mut E> {
        match self {
            Self::Eager(x) => Ok(x),
            Self::Lazy(x) => {
                *self = Self::Eager(eval(x)?);
                // Safety: we have the only mutable reference and have just
                //         overridden the variant to Self::Eager(...)
                let Self::Eager(x) = self else {
                    unsafe { std::hint::unreachable_unchecked() }
                };
                Ok(x)
            },
        }
    }
}

trait ValueTypeFrom: Sized {
    fn from_value(value: wasmparser::ValType) -> anyhow::Result<Self>;
    fn from_ref(ty: wasmparser::RefType) -> anyhow::Result<Self>;
}

impl ValueTypeFrom for ValueType {
    fn from_value(value: wasmparser::ValType) -> anyhow::Result<Self> {
        match value {
            wasmparser::ValType::I32 => Ok(Self::I32),
            wasmparser::ValType::I64 => Ok(Self::I64),
            wasmparser::ValType::F32 => Ok(Self::F32),
            wasmparser::ValType::F64 => Ok(Self::F64),
            wasmparser::ValType::V128 => {
                anyhow::bail!("v128 is not yet supported in the wasm_runtime_layer")
            },
            wasmparser::ValType::Ref(ty) => Self::from_ref(ty),
        }
    }

    fn from_ref(ty: wasmparser::RefType) -> anyhow::Result<Self> {
        if ty.is_func_ref() {
            Ok(Self::FuncRef)
        } else if ty.is_extern_ref() {
            Ok(Self::ExternRef)
        } else {
            anyhow::bail!("reference type {ty:?} is not yet supported in the wasm_runtime_layer")
        }
    }
}

trait FuncTypeFrom: Sized {
    fn from_parsed(value: &wasmparser::FuncType) -> anyhow::Result<Self>;
}

impl FuncTypeFrom for FuncType {
    fn from_parsed(value: &wasmparser::FuncType) -> anyhow::Result<Self> {
        let params = value
            .params()
            .iter()
            .copied()
            .map(ValueType::from_value)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let results = value
            .results()
            .iter()
            .copied()
            .map(ValueType::from_value)
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(Self::new(params, results))
    }
}

trait TableTypeFrom: Sized {
    fn from_parsed(value: &wasmparser::TableType) -> anyhow::Result<Self>;
}

impl TableTypeFrom for TableType {
    fn from_parsed(value: &wasmparser::TableType) -> anyhow::Result<Self> {
        Ok(Self::new(
            ValueType::from_ref(value.element_type)?,
            value.initial.try_into()?,
            match value.maximum {
                None => None,
                Some(maximum) => Some(maximum.try_into()?),
            },
        ))
    }
}

trait MemoryTypeFrom: Sized {
    fn from_parsed(value: &wasmparser::MemoryType) -> anyhow::Result<Self>;
}

impl MemoryTypeFrom for MemoryType {
    fn from_parsed(value: &wasmparser::MemoryType) -> anyhow::Result<Self> {
        if value.memory64 {
            anyhow::bail!("memory64 is not yet supported in the wasm_runtime_layer");
        }

        if value.shared {
            anyhow::bail!("shared memory is not yet supported in the wasm_runtime_layer");
        }

        Ok(Self::new(
            value.initial.try_into()?,
            match value.maximum {
                None => None,
                Some(maximum) => Some(maximum.try_into()?),
            },
        ))
    }
}

trait GlobalTypeFrom: Sized {
    fn from_parsed(value: &wasmparser::GlobalType) -> anyhow::Result<Self>;
}

impl GlobalTypeFrom for GlobalType {
    fn from_parsed(value: &wasmparser::GlobalType) -> anyhow::Result<Self> {
        Ok(Self::new(
            ValueType::from_value(value.content_type)?,
            value.mutable,
        ))
    }
}

fn web_assembly_module_new(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
    static WEB_ASSEMBLY_MODULE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    WEB_ASSEMBLY_MODULE.import(py, "js.WebAssembly.Module", "new")
}

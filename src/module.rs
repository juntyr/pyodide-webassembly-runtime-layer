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
        let mut memories = Vec::new();
        let mut tables = Vec::new();
        let mut globals = Vec::new();

        parser.parse_all(bytes).try_for_each(|payload| {
            match payload? {
                wasmparser::Payload::TypeSection(section) => {
                    for ty in section {
                        let ty = ty?;

                        let mut subtypes = ty.types();
                        let subtype = subtypes.next();

                        let ty = match (subtype, subtypes.next()) {
                            (Some(subtype), None) => match &subtype.composite_type.inner {
                                wasmparser::CompositeInnerType::Func(func_type) => FuncType::new(
                                    func_type
                                        .params()
                                        .iter()
                                        .copied()
                                        .map(ValueType::from_value),
                                    func_type
                                        .results()
                                        .iter()
                                        .copied()
                                        .map(ValueType::from_value),
                                ),
                                _ => unreachable!(),
                            },
                            _ => unimplemented!(),
                        };

                        types.push(ty);
                    }
                },
                wasmparser::Payload::FunctionSection(section) => {
                    for type_index in section {
                        let type_index = type_index?;

                        let ty = &types[type_index as usize];

                        functions.push(ty.clone());
                    }
                },
                wasmparser::Payload::TableSection(section) => {
                    for table in section {
                        let table = table?;
                        tables.push(TableType::from_parsed(&table.ty)?);
                    }
                },
                wasmparser::Payload::MemorySection(section) => {
                    for memory in section {
                        let memory = memory?;
                        memories.push(MemoryType::from_parsed(&memory)?);
                    }
                },
                wasmparser::Payload::GlobalSection(section) => {
                    for global in section {
                        let global = global?;
                        globals.push(GlobalType::from_parsed(global.ty));
                    }
                },
                wasmparser::Payload::TagSection(section) => {
                    for tag in section {
                        let tag = tag?;

                        #[cfg(feature = "tracing")]
                        tracing::trace!(?tag, "tag");
                        #[cfg(not(feature = "tracing"))]
                        let _ = tag;
                    }
                },
                wasmparser::Payload::ImportSection(section) => {
                    for import in section.into_imports() {
                        let import = import?;
                        let ty = match import.ty {
                            wasmparser::TypeRef::Func(index)
                            | wasmparser::TypeRef::FuncExact(index) => {
                                let sig = types[index as usize].clone().with_name(import.name);
                                functions.push(sig.clone());
                                ExternType::Func(sig)
                            },
                            wasmparser::TypeRef::Table(ty) => {
                                tables.push(TableType::from_parsed(&ty)?);
                                ExternType::Table(TableType::from_parsed(&ty)?)
                            },
                            wasmparser::TypeRef::Memory(ty) => {
                                memories.push(MemoryType::from_parsed(&ty)?);
                                ExternType::Memory(MemoryType::from_parsed(&ty)?)
                            },
                            wasmparser::TypeRef::Global(ty) => {
                                globals.push(GlobalType::from_parsed(ty));
                                ExternType::Global(GlobalType::from_parsed(ty))
                            },
                            wasmparser::TypeRef::Tag(_) => {
                                unimplemented!("WebAssembly.Tag is not yet supported")
                            },
                        };

                        imports.insert((import.module.to_string(), import.name.to_string()), ty);
                    }
                },
                wasmparser::Payload::ExportSection(section) => {
                    for export in section {
                        let export = export?;
                        let index = export.index as usize;
                        let ty = match export.kind {
                            wasmparser::ExternalKind::Func
                            | wasmparser::ExternalKind::FuncExact => {
                                ExternType::Func(functions[index].clone().with_name(export.name))
                            },
                            wasmparser::ExternalKind::Table => ExternType::Table(tables[index]),
                            wasmparser::ExternalKind::Memory => ExternType::Memory(memories[index]),
                            wasmparser::ExternalKind::Global => ExternType::Global(globals[index]),
                            wasmparser::ExternalKind::Tag => {
                                unimplemented!("WebAssembly.Tag is not yet supported")
                            },
                        };

                        exports.insert(export.name.to_string(), ty);
                    }
                },
                wasmparser::Payload::ElementSection(section) => {
                    for element in section {
                        let element = element?;

                        #[cfg(feature = "tracing")]
                        match element.kind {
                            wasmparser::ElementKind::Passive => tracing::debug!("passive"),
                            wasmparser::ElementKind::Active { .. } => tracing::debug!("active"),
                            wasmparser::ElementKind::Declared => tracing::debug!("declared"),
                        }
                        #[cfg(not(feature = "tracing"))]
                        let _ = element;
                    }
                },
                _ => (),
            }

            anyhow::Ok(())
        })?;

        Ok(Self { imports, exports })
    }
}

trait ValueTypeFrom {
    fn from_value(value: wasmparser::ValType) -> Self;
    fn from_ref(ty: wasmparser::RefType) -> Self;
}

impl ValueTypeFrom for ValueType {
    fn from_value(value: wasmparser::ValType) -> Self {
        match value {
            wasmparser::ValType::I32 => Self::I32,
            wasmparser::ValType::I64 => Self::I64,
            wasmparser::ValType::F32 => Self::F32,
            wasmparser::ValType::F64 => Self::F64,
            wasmparser::ValType::V128 => unimplemented!("v128 is not supported"),
            wasmparser::ValType::Ref(ty) => Self::from_ref(ty),
        }
    }

    fn from_ref(ty: wasmparser::RefType) -> Self {
        if ty.is_func_ref() {
            Self::FuncRef
        } else if ty.is_extern_ref() {
            Self::ExternRef
        } else {
            unimplemented!("unsupported reference type {ty:?}")
        }
    }
}

trait TableTypeFrom: Sized {
    fn from_parsed(value: &wasmparser::TableType) -> anyhow::Result<Self>;
}

impl TableTypeFrom for TableType {
    fn from_parsed(value: &wasmparser::TableType) -> anyhow::Result<Self> {
        Ok(Self::new(
            ValueType::from_ref(value.element_type),
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
            anyhow::bail!("memory64 is not yet supported");
        }

        if value.shared {
            anyhow::bail!("shared memory is not yet supported");
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

trait GlobalTypeFrom {
    fn from_parsed(value: wasmparser::GlobalType) -> Self;
}

impl GlobalTypeFrom for GlobalType {
    fn from_parsed(value: wasmparser::GlobalType) -> Self {
        Self::new(ValueType::from_value(value.content_type), value.mutable)
    }
}

fn web_assembly_module_new(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
    static WEB_ASSEMBLY_MODULE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    WEB_ASSEMBLY_MODULE.import(py, "js.WebAssembly.Module", "new")
}

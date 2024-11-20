use std::{error::Error, fmt};

use flagset::FlagSet;
use pyo3::{prelude::*, sync::GILOnceCell};

use crate::conversion::js_uint8_array_new;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedWasmFeatureExtensionError {
    pub required: FlagSet<WasmFeatureExtension>,
    pub supported: FlagSet<WasmFeatureExtension>,
}

impl UnsupportedWasmFeatureExtensionError {
    pub fn check_support(py: Python, bytes: &[u8]) -> Result<Result<(), Self>, PyErr> {
        let err = Self {
            required: WasmFeatureExtension::required(bytes),
            supported: *WasmFeatureExtension::supported(py)?,
        };

        if (err.required & (!err.supported)).is_empty() {
            return Ok(Ok(()));
        }

        Ok(Err(err))
    }
}

impl fmt::Display for UnsupportedWasmFeatureExtensionError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            fmt,
            "A WASM module requires the following feature extensions, which are not supported by \
             your browser:"
        )?;
        writeln!(fmt)?;

        for missing in self.required & (!self.supported) {
            writeln!(fmt, " - {missing}")?;
        }

        writeln!(fmt)?;
        writeln!(
            fmt,
            "Please check out https://webassembly.org/features/ to see if a newer version of your \
             browser already supports these features."
        )
    }
}

impl Error for UnsupportedWasmFeatureExtensionError {}

flagset::flags! {
    #[non_exhaustive]
    pub enum WasmFeatureExtension: u64 {
        BulkMemory,
        Exceptions,
        ExtendedConst,
        FunctionReferences,
        GC,
        Memory64,
        MultiMemory,
        MultiValue,
        MutableGlobal,
        ReferenceTypes,
        RelaxedSimd,
        SaturatingFloatToInt,
        SignExtension,
        Simd,
        TailCall,
        Threads,
    }
}

impl From<WasmFeatureExtension> for wasmparser::WasmFeatures {
    fn from(extension: WasmFeatureExtension) -> Self {
        match extension {
            WasmFeatureExtension::BulkMemory => Self::BULK_MEMORY,
            WasmFeatureExtension::Exceptions => Self::EXCEPTIONS,
            WasmFeatureExtension::ExtendedConst => Self::EXTENDED_CONST,
            WasmFeatureExtension::FunctionReferences => Self::FUNCTION_REFERENCES,
            WasmFeatureExtension::GC => Self::GC,
            WasmFeatureExtension::Memory64 => Self::MEMORY64,
            WasmFeatureExtension::MultiMemory => Self::MULTI_MEMORY,
            WasmFeatureExtension::MultiValue => Self::MULTI_VALUE,
            WasmFeatureExtension::MutableGlobal => Self::MUTABLE_GLOBAL,
            WasmFeatureExtension::ReferenceTypes => Self::REFERENCE_TYPES,
            WasmFeatureExtension::RelaxedSimd => Self::RELAXED_SIMD,
            WasmFeatureExtension::SaturatingFloatToInt => Self::SATURATING_FLOAT_TO_INT,
            WasmFeatureExtension::SignExtension => Self::SIGN_EXTENSION,
            WasmFeatureExtension::Simd => Self::SIMD,
            WasmFeatureExtension::TailCall => Self::TAIL_CALL,
            WasmFeatureExtension::Threads => Self::THREADS,
        }
    }
}

impl WasmFeatureExtension {
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn required(bytes: &[u8]) -> FlagSet<Self> {
        let mut required: FlagSet<_> = FlagSet::default();

        required.extend(
            FlagSet::<Self>::full()
                .into_iter()
                .filter(|extension| Self::requires_features(bytes, (*extension).into())),
        );

        required
    }

    pub fn supported(py: Python) -> Result<&'static FlagSet<Self>, PyErr> {
        static SUPPORTED_FEATURES: GILOnceCell<FlagSet<WasmFeatureExtension>> = GILOnceCell::new();

        SUPPORTED_FEATURES.get_or_try_init(py, || {
            let mut supported = FlagSet::default();

            for extension in FlagSet::<Self>::full() {
                if extension.check_if_supported(py)? {
                    supported |= extension;
                }
            }

            Ok(supported)
        })
    }

    pub fn check_if_supported(self, py: Python) -> Result<bool, PyErr> {
        let canary = self.canary_bytes();

        if matches!(self, Self::MultiMemory) {
            Self::try_create_wasm_module_from_bytes(py, canary)
        } else {
            Self::try_validate_wasm_bytes(py, canary)
        }
    }

    const fn canary_bytes(self) -> &'static [u8] {
        // The WASM feature detection mechanism and the detector WASM modules
        // are adapted from the Google Chrome Team's `wasm-feature-detect`
        // repository, which is released under the Apache-2.0 License.
        // https://github.com/GoogleChromeLabs/wasm-feature-detect/tree/8bfe6691b0749b53d605f3220f15e68751c4b5b6/src/detectors
        //
        // The detector modules have been compiled from *.wat to *.wasm
        // using binaryen's wasm-as.
        match self {
            Self::BulkMemory => include_bytes!("bulk-memory.wasm"),
            Self::Exceptions => include_bytes!("exceptions.wasm"),
            Self::ExtendedConst => include_bytes!("extended-const.wasm"),
            Self::FunctionReferences => include_bytes!("function-references.wasm"),
            Self::GC => include_bytes!("gc.wasm"),
            Self::Memory64 => include_bytes!("memory64.wasm"),
            Self::MultiMemory => include_bytes!("multi-memory.wasm"),
            Self::MultiValue => include_bytes!("multi-value.wasm"),
            Self::MutableGlobal => include_bytes!("mutable-global.wasm"),
            Self::ReferenceTypes => include_bytes!("reference-types.wasm"),
            Self::RelaxedSimd => include_bytes!("relaxed-simd.wasm"),
            Self::SaturatingFloatToInt => include_bytes!("saturating-float-to-int.wasm"),
            Self::SignExtension => include_bytes!("sign-extension.wasm"),
            Self::Simd => include_bytes!("simd.wasm"),
            Self::TailCall => include_bytes!("tail-call.wasm"),
            Self::Threads => include_bytes!("threads.wasm"),
        }
    }

    fn requires_features(bytes: &[u8], features: wasmparser::WasmFeatures) -> bool {
        wasmparser::Validator::new_with_features(!features)
            .validate_all(bytes)
            .is_err()
    }

    fn try_validate_wasm_bytes(py: Python, bytes: &[u8]) -> Result<bool, PyErr> {
        let buffer = js_uint8_array_new(py)?.call1((bytes,))?;
        let valid = web_assembly_validate(py)?.call1((buffer,))?.extract()?;
        Ok(valid)
    }

    fn try_create_wasm_module_from_bytes(py: Python, bytes: &[u8]) -> Result<bool, PyErr> {
        let buffer = js_uint8_array_new(py)?.call1((bytes,))?;
        let module = web_assembly_module_new(py)?.call1((buffer,));
        Ok(module.is_ok())
    }
}

impl fmt::Display for WasmFeatureExtension {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(
            fmt,
            "{}",
            match self {
                Self::BulkMemory => "bulk-memory",
                Self::Exceptions => "exceptions",
                Self::ExtendedConst => "extended-const",
                Self::FunctionReferences => "function-references",
                Self::GC => "gc",
                Self::Memory64 => "memory64",
                Self::MultiMemory => "multi-memory",
                Self::MultiValue => "multi-value",
                Self::MutableGlobal => "mutable-global",
                Self::ReferenceTypes => "reference-types",
                Self::RelaxedSimd => "relaxed-simd",
                Self::SaturatingFloatToInt => "saturating-float-to-int",
                Self::SignExtension => "sign-extension",
                Self::Simd => "simd",
                Self::TailCall => "tail-call",
                Self::Threads => "threads",
            }
        )
    }
}

fn web_assembly_validate(py: Python) -> Result<&Bound<PyAny>, PyErr> {
    static WEB_ASSEMBLY_VALIDATE: GILOnceCell<Py<PyAny>> = GILOnceCell::new();
    WEB_ASSEMBLY_VALIDATE.import(py, "js.WebAssembly", "validate")
}

fn web_assembly_module_new(py: Python) -> Result<&Bound<PyAny>, PyErr> {
    static WEB_ASSEMBLY_MODULE: GILOnceCell<Py<PyAny>> = GILOnceCell::new();
    WEB_ASSEMBLY_MODULE.import(py, "js.WebAssembly.Module", "new")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_features() {
        for feature in FlagSet::<WasmFeatureExtension>::full() {
            let required = WasmFeatureExtension::required(feature.canary_bytes());

            let check = match feature {
                // the function-references feature depends on reference-types
                // FIXME: remove the dependency on bulk-memory
                WasmFeatureExtension::FunctionReferences => {
                    FlagSet::from(feature)
                        | WasmFeatureExtension::ReferenceTypes
                        | WasmFeatureExtension::BulkMemory
                },
                // the relaxed-simd feature depends on simd
                WasmFeatureExtension::RelaxedSimd => {
                    FlagSet::from(feature) | WasmFeatureExtension::Simd
                },
                // otherwise, every feature should only require itself
                _ => FlagSet::from(feature),
            };

            assert_eq!(
                required, check,
                "{feature} should only require {check:?}, but needs {required:?}"
            );
        }
    }
}

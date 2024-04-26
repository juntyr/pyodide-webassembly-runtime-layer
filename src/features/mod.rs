use std::{error::Error, fmt, sync::OnceLock};

use flagset::FlagSet;
use pyo3::{intern, prelude::*};

use crate::conversion::js_uint8_array;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedWasmFeatureExtensionError {
    pub required: FlagSet<WasmFeatureExtension>,
    pub supported: FlagSet<WasmFeatureExtension>,
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

    pub fn supported() -> &'static FlagSet<Self> {
        static SUPPORTED_FEATURES: OnceLock<FlagSet<WasmFeatureExtension>> = OnceLock::new();

        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        SUPPORTED_FEATURES.get_or_init(|| {
            Python::with_gil(|py| {
                let mut supported = FlagSet::default();

                supported.extend(
                    FlagSet::<Self>::full()
                        .into_iter()
                        .filter(|extension| extension.check_if_supported(py).unwrap()),
                );

                supported
            })
        })
    }

    pub fn check_if_supported(self, py: Python) -> Result<bool, anyhow::Error> {
        // The WASM feature detection mechanism and the detector WASM modules
        // are adapted from the Google Chrome Team's `wasm-feature-detect`
        // repository, which is released under the Apache-2.0 License.
        // https://github.com/GoogleChromeLabs/wasm-feature-detect/tree/5e491be2d5808948a0706234ab1475c88cedc069/src/detectors
        //
        // The detector modules have been compiled from *.wat to *.wasm
        // using wabt.
        match self {
            Self::BulkMemory => {
                Self::try_validate_wasm_bytes(py, include_bytes!("bulk-memory.wasm"))
            },
            Self::Exceptions => {
                Self::try_validate_wasm_bytes(py, include_bytes!("exceptions.wasm"))
            },
            Self::ExtendedConst => {
                Self::try_validate_wasm_bytes(py, include_bytes!("extended-const.wasm"))
            },
            Self::GC => Self::try_validate_wasm_bytes(py, include_bytes!("gc.wasm")),
            Self::Memory64 => Self::try_validate_wasm_bytes(py, include_bytes!("memory64.wasm")),
            Self::MultiMemory => {
                Self::try_create_wasm_module_from_bytes(py, include_bytes!("multi-memory.wasm"))
            },
            Self::MultiValue => {
                Self::try_validate_wasm_bytes(py, include_bytes!("multi-value.wasm"))
            },
            Self::MutableGlobal => {
                Self::try_validate_wasm_bytes(py, include_bytes!("mutable-global.wasm"))
            },
            Self::ReferenceTypes => {
                Self::try_validate_wasm_bytes(py, include_bytes!("reference-types.wasm"))
            },
            Self::RelaxedSimd => {
                Self::try_validate_wasm_bytes(py, include_bytes!("relaxed-simd.wasm"))
            },
            Self::SaturatingFloatToInt => {
                Self::try_validate_wasm_bytes(py, include_bytes!("saturating-float-to-int.wasm"))
            },
            Self::SignExtension => {
                Self::try_validate_wasm_bytes(py, include_bytes!("sign-extension.wasm"))
            },
            Self::Simd => Self::try_validate_wasm_bytes(py, include_bytes!("simd.wasm")),
            Self::TailCall => Self::try_validate_wasm_bytes(py, include_bytes!("tail-call.wasm")),
            Self::Threads => Self::try_validate_wasm_bytes(py, include_bytes!("threads.wasm")),
        }
    }

    fn requires_features(bytes: &[u8], features: wasmparser::WasmFeatures) -> bool {
        wasmparser::Validator::new_with_features(features)
            .validate_all(bytes)
            .is_err()
    }

    fn try_validate_wasm_bytes(py: Python, bytes: &[u8]) -> anyhow::Result<bool> {
        let buffer = js_uint8_array(py).call_method1(py, intern!(py, "new"), (bytes,))?;
        let valid = web_assembly_validate(py)
            .call1(py, (buffer,))?
            .extract(py)?;
        Ok(valid)
    }

    fn try_create_wasm_module_from_bytes(py: Python, bytes: &[u8]) -> anyhow::Result<bool> {
        let buffer = js_uint8_array(py).call_method1(py, intern!(py, "new"), (bytes,))?;
        let module = web_assembly_module(py).call_method1(py, intern!(py, "new"), (buffer,));
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

fn web_assembly_validate(py: Python) -> &'static Py<PyAny> {
    static WEB_ASSEMBLY_VALIDATE: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    WEB_ASSEMBLY_VALIDATE.get_or_init(|| {
        py.import(intern!(py, "js"))
            .unwrap()
            .getattr(intern!(py, "WebAssembly"))
            .unwrap()
            .getattr(intern!(py, "validate"))
            .unwrap()
            .into_py(py)
    })
}

fn web_assembly_module(py: Python) -> &'static Py<PyAny> {
    static WEB_ASSEMBLY_MODULE: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    WEB_ASSEMBLY_MODULE.get_or_init(|| {
        py.import(intern!(py, "js"))
            .unwrap()
            .getattr(intern!(py, "WebAssembly"))
            .unwrap()
            .getattr(intern!(py, "Module"))
            .unwrap()
            .into_py(py)
    })
}

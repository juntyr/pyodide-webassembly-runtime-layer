use std::{error::Error, fmt, sync::OnceLock};

use flagset::FlagSet;
use pyo3::{intern, prelude::*};

use crate::conversion::js_uint8_array;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedWasmFeatureExtensionError {
    pub required: FlagSet<WasmFeatureExtensions>,
    pub supported: FlagSet<WasmFeatureExtensions>,
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
    pub enum WasmFeatureExtensions: u64 {
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

impl WasmFeatureExtensions {
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn required(bytes: &[u8]) -> FlagSet<Self> {
        let mut features = FlagSet::default();

        Self::BulkMemory.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    bulk_memory: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::Exceptions.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    exceptions: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::ExtendedConst.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    extended_const: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::GC.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    gc: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::Memory64.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    memory64: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::MultiMemory.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    multi_memory: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::MultiValue.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    multi_value: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::MutableGlobal.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    mutable_global: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::ReferenceTypes.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    reference_types: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::RelaxedSimd.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    relaxed_simd: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::SaturatingFloatToInt.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    saturating_float_to_int: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::SignExtension.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    sign_extension: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::Simd.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    simd: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::TailCall.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    tail_call: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );
        Self::Threads.add_if(
            &mut features,
            Self::requires_features(
                bytes,
                wasmparser::WasmFeatures {
                    threads: false,
                    ..wasmparser::WasmFeatures::all()
                },
            ),
        );

        features
    }

    pub fn supported() -> &'static FlagSet<Self> {
        static SUPPORTED_FEATURES: OnceLock<FlagSet<WasmFeatureExtensions>> = OnceLock::new();

        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        SUPPORTED_FEATURES.get_or_init(|| {
            Python::with_gil(|py| {
                let mut features = FlagSet::default();

                // The WASM feature detection mechanism and the detector WASM modules
                // are adapted from the Google Chrome Team's `wasm-feature-detect`
                // repository, which is released under the Apache-2.0 License.
                // https://github.com/GoogleChromeLabs/wasm-feature-detect/tree/5e491be2d5808948a0706234ab1475c88cedc069/src/detectors
                //
                // The detector modules have been compiled from *.wat to *.wasm
                // using wabt.
                Self::BulkMemory.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("bulk-memory.wasm")).unwrap(),
                );
                Self::Exceptions.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("exceptions.wasm")).unwrap(),
                );
                Self::ExtendedConst.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("extended-const.wasm"))
                        .unwrap(),
                );
                Self::GC.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("gc.wasm")).unwrap(),
                );
                Self::Memory64.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("memory64.wasm")).unwrap(),
                );
                Self::MultiMemory.add_if(
                    &mut features,
                    Self::try_create_wasm_module_from_bytes(
                        py,
                        include_bytes!("multi-memory.wasm"),
                    )
                    .unwrap(),
                );
                Self::MultiValue.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("multi-value.wasm")).unwrap(),
                );
                Self::MutableGlobal.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("mutable-global.wasm"))
                        .unwrap(),
                );
                Self::ReferenceTypes.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("reference-types.wasm"))
                        .unwrap(),
                );
                Self::RelaxedSimd.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("relaxed-simd.wasm")).unwrap(),
                );
                Self::SaturatingFloatToInt.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(
                        py,
                        include_bytes!("saturating-float-to-int.wasm"),
                    )
                    .unwrap(),
                );
                Self::SignExtension.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("sign-extension.wasm"))
                        .unwrap(),
                );
                Self::Simd.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("simd.wasm")).unwrap(),
                );
                Self::TailCall.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("tail-call.wasm")).unwrap(),
                );
                Self::Threads.add_if(
                    &mut features,
                    Self::try_validate_wasm_bytes(py, include_bytes!("threads.wasm")).unwrap(),
                );

                features
            })
        })
    }

    fn add_if(self, features: &mut FlagSet<Self>, cond: bool) {
        if cond {
            *features |= self;
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

impl fmt::Display for WasmFeatureExtensions {
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

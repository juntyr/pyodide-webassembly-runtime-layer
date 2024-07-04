use std::{error::Error, fmt};

use flagset::FlagSet;
use pyo3::{intern, prelude::*, sync::GILOnceCell};

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

impl WasmFeatureExtension {
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn required(bytes: &[u8]) -> FlagSet<Self> {
        let mut required: FlagSet<_> = FlagSet::default();

        required.extend(
            FlagSet::<Self>::full()
                .into_iter()
                .filter(|extension| extension.requires_feature(bytes)),
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

    pub fn check_if_supported(self, py: Python) -> Result<bool, anyhow::Error> {
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
        // https://github.com/GoogleChromeLabs/wasm-feature-detect/tree/5e491be2d5808948a0706234ab1475c88cedc069/src/detectors
        //
        // The detector modules have been compiled from *.wat to *.wasm
        // using wabt's wat2wasm.
        match self {
            Self::BulkMemory => include_bytes!("bulk-memory.wasm"),
            Self::Exceptions => include_bytes!("exceptions.wasm"),
            Self::ExtendedConst => include_bytes!("extended-const.wasm"),
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

    fn requires_feature(self, bytes: &[u8]) -> bool {
        let all_except = match self {
            Self::BulkMemory => wasmparser::WasmFeatures {
                bulk_memory: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::Exceptions => wasmparser::WasmFeatures {
                exceptions: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::ExtendedConst => wasmparser::WasmFeatures {
                extended_const: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::GC => wasmparser::WasmFeatures {
                gc: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::Memory64 => wasmparser::WasmFeatures {
                memory64: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::MultiMemory => wasmparser::WasmFeatures {
                multi_memory: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::MultiValue => wasmparser::WasmFeatures {
                multi_value: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::MutableGlobal => wasmparser::WasmFeatures {
                mutable_global: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::ReferenceTypes => wasmparser::WasmFeatures {
                reference_types: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::RelaxedSimd => wasmparser::WasmFeatures {
                relaxed_simd: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::SaturatingFloatToInt => wasmparser::WasmFeatures {
                saturating_float_to_int: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::SignExtension => wasmparser::WasmFeatures {
                sign_extension: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::Simd => wasmparser::WasmFeatures {
                simd: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::TailCall => wasmparser::WasmFeatures {
                tail_call: false,
                ..wasmparser::WasmFeatures::all()
            },
            Self::Threads => wasmparser::WasmFeatures {
                threads: false,
                ..wasmparser::WasmFeatures::all()
            },
        };

        wasmparser::Validator::new_with_features(all_except)
            .validate_all(bytes)
            .is_err()
    }

    fn try_validate_wasm_bytes(py: Python, bytes: &[u8]) -> anyhow::Result<bool> {
        let buffer = js_uint8_array(py).call_method1(py, intern!(py, "new"), (bytes,))?;
        let valid = web_assembly_validate(py)?
            .call1(py, (buffer,))?
            .extract(py)?;
        Ok(valid)
    }

    fn try_create_wasm_module_from_bytes(py: Python, bytes: &[u8]) -> anyhow::Result<bool> {
        let buffer = js_uint8_array(py).call_method1(py, intern!(py, "new"), (bytes,))?;
        let module = web_assembly_module(py)?.call_method1(py, intern!(py, "new"), (buffer,));
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

fn web_assembly_validate(py: Python) -> Result<&'static Py<PyAny>, PyErr> {
    static WEB_ASSEMBLY_VALIDATE: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

    WEB_ASSEMBLY_VALIDATE.get_or_try_init(py, || {
        Ok(py
            .import(intern!(py, "js"))?
            .getattr(intern!(py, "WebAssembly"))?
            .getattr(intern!(py, "validate"))?
            .into_py(py))
    })
}

fn web_assembly_module(py: Python) -> Result<&'static Py<PyAny>, PyErr> {
    static WEB_ASSEMBLY_MODULE: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

    WEB_ASSEMBLY_MODULE.get_or_try_init(py, || {
        Ok(py
            .import(intern!(py, "js"))?
            .getattr(intern!(py, "WebAssembly"))?
            .getattr(intern!(py, "Module"))?
            .into_py(py))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_features() {
        for feature in FlagSet::<WasmFeatureExtension>::full() {
            let required = WasmFeatureExtension::required(feature.canary_bytes());

            let check = match feature {
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

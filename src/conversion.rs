use std::sync::OnceLock;

use pyo3::{intern, prelude::*, types::IntoPyDict};
use wasm_runtime_layer::{
    backend::{Extern, Value},
    ValueType,
};

use crate::{Engine, ExternRef};

/// Converts a Rust type to Python
pub trait ToPy {
    /// Convert this value to Python
    fn to_py(&self, py: Python) -> Py<PyAny>;
}

impl ToPy for Value<Engine> {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(ty = ?self.ty(), "Value::to_py");

        match self {
            Self::I32(v) => v.to_object(py),
            // WebAssembly explicitly requires all i64's to be a BigInt
            Self::I64(v) => i64_to_bigint(py, *v),
            Self::F32(v) => v.to_object(py),
            Self::F64(v) => v.to_object(py),
            Self::FuncRef(None) | Self::ExternRef(None) => py.None(),
            Self::FuncRef(Some(func)) => func.to_py(py),
            Self::ExternRef(Some(r#ref)) => r#ref.to_py(py),
        }
    }
}

impl ToPy for Extern<Engine> {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!("Extern::to_py");

        match self {
            Self::Global(v) => v.to_py(py),
            Self::Table(v) => v.to_py(py),
            Self::Memory(v) => v.to_py(py),
            Self::Func(v) => v.to_py(py),
        }
    }
}

pub trait ValueExt: Sized {
    /// Convert a value to its type
    fn ty(&self) -> ValueType;

    /// Convert the [`PyAny`] value into a Value of the supplied type
    fn from_py_typed(py: Python, value: Py<PyAny>, ty: ValueType) -> anyhow::Result<Self>;
}

impl ValueExt for Value<Engine> {
    /// Convert a value to its type
    fn ty(&self) -> ValueType {
        match self {
            Self::I32(_) => ValueType::I32,
            Self::I64(_) => ValueType::I64,
            Self::F32(_) => ValueType::F32,
            Self::F64(_) => ValueType::F64,
            Self::FuncRef(_) => ValueType::FuncRef,
            Self::ExternRef(_) => ValueType::ExternRef,
        }
    }

    fn from_py_typed(py: Python, value: Py<PyAny>, ty: ValueType) -> anyhow::Result<Self> {
        match ty {
            ValueType::I32 => Ok(Self::I32(value.extract(py)?)),
            ValueType::I64 => Ok(Self::I64(value.extract(py)?)),
            ValueType::F32 => Ok(Self::F32(value.extract(py)?)),
            ValueType::F64 => Ok(Self::F64(value.extract(py)?)),
            ValueType::ExternRef => {
                if value.is_none(py) {
                    Ok(Self::ExternRef(None))
                } else {
                    Ok(Self::ExternRef(Some(ExternRef::from_exported_externref(
                        value,
                    ))))
                }
            },
            ValueType::FuncRef => {
                if value.is_none(py) {
                    Ok(Self::FuncRef(None))
                } else {
                    anyhow::bail!(
                        "conversion to a function outside of a module export is not permitted as \
                         its type signature is unknown"
                    )
                }
            },
        }
    }
}

pub trait ValueTypeExt {
    /// Converts this type into the canonical ABI kind
    ///
    /// See: <https://webassembly.github.io/spec/js-api/#globals>
    fn as_js_descriptor(&self) -> &str;
}

impl ValueTypeExt for ValueType {
    fn as_js_descriptor(&self) -> &str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::FuncRef => "anyfunc",
            Self::ExternRef => "externref",
        }
    }
}

#[must_use]
fn i64_to_bigint(py: Python, x: i64) -> Py<PyAny> {
    fn bigint(py: Python) -> &Py<PyAny> {
        static BIG_INT: OnceLock<Py<PyAny>> = OnceLock::new();
        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        BIG_INT.get_or_init(|| {
            py.import(intern!(py, "js"))
                .unwrap()
                .getattr(intern!(py, "BigInt"))
                .unwrap()
                .into_py(py)
        })
    }

    // Conversion from i64 to BigInt cannot fail
    bigint(py).call1(py, (x,)).unwrap()
}

pub fn uint8_array(py: Python) -> &'static Py<PyAny> {
    static UINT8_ARRAY: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    UINT8_ARRAY.get_or_init(|| {
        py.import(intern!(py, "js"))
            .unwrap()
            .getattr(intern!(py, "Uint8Array"))
            .unwrap()
            .into_py(py)
    })
}

/// Check if `object` is an instance of the JavaScript class with `constructor`.
pub fn instanceof(py: Python, object: &Py<PyAny>, constructor: &Py<PyAny>) -> Result<bool, PyErr> {
    fn is_instance_of(py: Python) -> &Py<PyAny> {
        static IS_INSTANCE_OF: OnceLock<Py<PyAny>> = OnceLock::new();
        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        IS_INSTANCE_OF.get_or_init(|| {
            py.import(intern!(py, "pyodide"))
                .unwrap()
                .getattr(intern!(py, "code"))
                .unwrap()
                .getattr(intern!(py, "run_js"))
                .unwrap()
                .call1((
                    "function isInstanceOf(object, constructor){ return (object instanceof \
                     constructor); } isInstanceOf",
                ))
                .unwrap()
                .into_py(py)
        })
    }

    is_instance_of(py)
        .call1(py, (object, constructor))?
        .extract(py)
}

pub fn create_js_object(py: Python) -> Result<Py<PyAny>, PyErr> {
    fn js_object_new(py: Python) -> &'static Py<PyAny> {
        static JS_OBJECT_NEW: OnceLock<Py<PyAny>> = OnceLock::new();
        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        JS_OBJECT_NEW.get_or_init(|| {
            py.import(intern!(py, "js"))
                .unwrap()
                .getattr(intern!(py, "Object"))
                .unwrap()
                .getattr(intern!(py, "new"))
                .unwrap()
                .into_py(py)
        })
    }

    js_object_new(py).call0(py)
}

pub fn py_to_js_proxy(py: Python, object: impl IntoPy<Py<PyAny>>) -> Result<Py<PyAny>, PyErr> {
    fn to_js(py: Python) -> &'static Py<PyAny> {
        static TO_JS: OnceLock<Py<PyAny>> = OnceLock::new();
        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        TO_JS.get_or_init(|| {
            py.import(intern!(py, "pyodide"))
                .unwrap()
                .getattr(intern!(py, "ffi"))
                .unwrap()
                .getattr(intern!(py, "to_js"))
                .unwrap()
                .into_py(py)
        })
    }

    to_js(py).call(
        py,
        (object,),
        Some([(intern!(py, "create_pyproxies"), true)].into_py_dict(py)),
    )
}

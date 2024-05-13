use pyo3::{intern, prelude::*, sync::GILOnceCell, types::IntoPyDict};
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
            // Pyodide auto-converts BigInts, so we wrap it in an Object
            // Conversion from an i64 to a BigInt that is wrapped in an Object cannot fail
            Self::I64(v) => i64_to_js_bigint(py, *v).unwrap().unbind(),
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
    fn from_py_typed(value: Bound<PyAny>, ty: ValueType) -> anyhow::Result<Self>;
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

    fn from_py_typed(value: Bound<PyAny>, ty: ValueType) -> anyhow::Result<Self> {
        match ty {
            ValueType::I32 => Ok(Self::I32(value.extract()?)),
            // Try to unwrap a number, BigInt, or Object-wrapped BigInt
            ValueType::I64 => Ok(Self::I64(try_i64_from_js_bigint(value)?)),
            ValueType::F32 => Ok(Self::F32(value.extract()?)),
            ValueType::F64 => Ok(Self::F64(value.extract()?)),
            ValueType::ExternRef => {
                if value.is_none() {
                    Ok(Self::ExternRef(None))
                } else {
                    Ok(Self::ExternRef(Some(ExternRef::from_exported_externref(
                        value,
                    ))))
                }
            },
            ValueType::FuncRef => {
                if value.is_none() {
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

fn i64_to_js_bigint(py: Python, v: i64) -> Result<Bound<PyAny>, PyErr> {
    fn object_wrapped_bigint(py: Python) -> Result<&Bound<PyAny>, PyErr> {
        static OBJECT_WRAPPED_BIGINT: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

        OBJECT_WRAPPED_BIGINT
            .get_or_try_init(py, || {
                Ok(py
                    .import_bound(intern!(py, "pyodide"))?
                    .getattr(intern!(py, "code"))?
                    .getattr(intern!(py, "run_js"))?
                    .call1((
                        "function objectWrappedBigInt(v){ return Object(BigInt(v)); } \
                         objectWrappedBigInt",
                    ))?
                    .into_py(py))
            })
            .map(|x| x.bind(py))
    }

    object_wrapped_bigint(py)?.call1((v,))
}

fn try_i64_from_js_bigint(v: Bound<PyAny>) -> Result<i64, PyErr> {
    fn js_bigint(py: Python) -> Result<&Bound<PyAny>, PyErr> {
        static JS_BIG_INT: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

        JS_BIG_INT
            .get_or_try_init(py, || {
                Ok(py
                    .import_bound(intern!(py, "js"))?
                    .getattr(intern!(py, "BigInt"))?
                    .into_py(py))
            })
            .map(|x| x.bind(py))
    }

    // First wrap inside a BigInt to force coersion, then try to convert into an i64
    js_bigint(v.py())?.call1((v,))?.extract()
}

pub fn js_uint8_array(py: Python) -> Result<&Bound<PyAny>, PyErr> {
    static JS_UINT8_ARRAY: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

    JS_UINT8_ARRAY
        .get_or_try_init(py, || {
            Ok(py
                .import_bound(intern!(py, "js"))?
                .getattr(intern!(py, "Uint8Array"))?
                .into_py(py))
        })
        .map(|x| x.bind(py))
}

/// Check if `object` is an instance of the JavaScript class with `constructor`.
pub fn instanceof(object: &Bound<PyAny>, constructor: &Bound<PyAny>) -> Result<bool, PyErr> {
    fn is_instance_of(py: Python) -> Result<&Bound<PyAny>, PyErr> {
        static IS_INSTANCE_OF: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

        IS_INSTANCE_OF
            .get_or_try_init(py, || {
                Ok(py
                    .import_bound(intern!(py, "pyodide"))?
                    .getattr(intern!(py, "code"))?
                    .getattr(intern!(py, "run_js"))?
                    .call1((
                        "function isInstanceOf(object, constructor){ return (object instanceof \
                         constructor); } isInstanceOf",
                    ))?
                    .into_py(py))
            })
            .map(|x| x.bind(py))
    }

    is_instance_of(object.py())?
        .call1((object, constructor))?
        .extract()
}

pub fn create_js_object(py: Python) -> Result<Bound<PyAny>, PyErr> {
    fn js_object_new(py: Python) -> Result<&Bound<PyAny>, PyErr> {
        static JS_OBJECT_NEW: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

        JS_OBJECT_NEW
            .get_or_try_init(py, || {
                Ok(py
                    .import_bound(intern!(py, "js"))?
                    .getattr(intern!(py, "Object"))?
                    .getattr(intern!(py, "new"))?
                    .into_py(py))
            })
            .map(|x| x.bind(py))
    }

    js_object_new(py)?.call0()
}

pub fn py_to_js_proxy<T>(object: Bound<T>) -> Result<Bound<PyAny>, PyErr> {
    fn to_js(py: Python) -> Result<&Bound<PyAny>, PyErr> {
        static TO_JS: GILOnceCell<Py<PyAny>> = GILOnceCell::new();

        TO_JS
            .get_or_try_init(py, || {
                Ok(py
                    .import_bound(intern!(py, "pyodide"))?
                    .getattr(intern!(py, "ffi"))?
                    .getattr(intern!(py, "to_js"))?
                    .into_py(py))
            })
            .map(|x| x.bind(py))
    }

    let py = object.py();

    to_js(py)?.call(
        (object,),
        Some(&[(intern!(py, "create_pyproxies"), true)].into_py_dict_bound(py)),
    )
}

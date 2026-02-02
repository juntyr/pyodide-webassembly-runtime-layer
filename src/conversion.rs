use std::convert::Infallible;

use pyo3::{
    exceptions::PyRuntimeError, intern, prelude::*, sync::PyOnceLock, types::IntoPyDict, PyTypeInfo,
};
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
        fn into_pyobject_infallible<
            'py,
            T: IntoPyObject<'py, Output = Bound<'py, S>, Error = Infallible>,
            S,
        >(
            py: Python<'py>,
            x: T,
        ) -> Py<PyAny> {
            match x.into_pyobject(py) {
                Ok(x) => x.into_any().unbind(),
                Err(e) => match e {},
            }
        }

        #[cfg(feature = "tracing")]
        tracing::trace!(ty = ?self.ty(), "Value::to_py");

        match self {
            Self::I32(v) => into_pyobject_infallible(py, v),
            // WebAssembly explicitly requires all i64's to be a BigInt
            // Pyodide auto-converts BigInts, so we wrap it in an Object
            Self::I64(v) => i64_to_js_bigint(py, *v).unbind(),
            Self::F32(v) => into_pyobject_infallible(py, v),
            Self::F64(v) => into_pyobject_infallible(py, v),
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
    fn from_py_typed(value: Bound<PyAny>, ty: ValueType) -> Result<Self, PyErr>;
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

    fn from_py_typed(value: Bound<PyAny>, ty: ValueType) -> Result<Self, PyErr> {
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
                    Err(PyRuntimeError::new_err(
                        "conversion to a function outside of a module export is not permitted as \
                         its type signature is unknown",
                    ))
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

fn i64_to_js_bigint(py: Python, v: i64) -> Bound<PyAny> {
    fn object_wrapped_bigint(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
        static OBJECT_WRAPPED_BIGINT: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

        OBJECT_WRAPPED_BIGINT
            .get_or_try_init(py, || {
                Ok(py
                    .import(intern!(py, "pyodide"))?
                    .getattr(intern!(py, "code"))?
                    .getattr(intern!(py, "run_js"))?
                    .call1((
                        "function objectWrappedBigInt(v){ return Object(BigInt(v)); } \
                         objectWrappedBigInt",
                    ))?
                    .unbind())
            })
            .map(|x| x.bind(py))
    }

    let bigint = (|| object_wrapped_bigint(py)?.call1((v,)))();

    bigint.expect("conversion from i64 to Object(BigInt(v)) should not fail")
}

fn try_i64_from_js_bigint(v: Bound<PyAny>) -> Result<i64, PyErr> {
    fn js_bigint(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
        static JS_BIG_INT: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        JS_BIG_INT.import(py, "js", "BigInt")
    }

    // First wrap inside a BigInt to force coersion, then try to convert into an i64
    js_bigint(v.py())?.call1((v,))?.extract()
}

pub fn js_uint8_array_new(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
    static JS_UINT8_ARRAY_NEW: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    JS_UINT8_ARRAY_NEW.import(py, "js.Uint8Array", "new")
}

/// Check if `object` is an instance of the JavaScript class with `constructor`.
pub fn instanceof(object: &Bound<PyAny>, constructor: &Bound<PyAny>) -> Result<bool, PyErr> {
    fn is_instance_of(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
        static IS_INSTANCE_OF: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

        IS_INSTANCE_OF
            .get_or_try_init(py, || {
                Ok(py
                    .import(intern!(py, "pyodide"))?
                    .getattr(intern!(py, "code"))?
                    .getattr(intern!(py, "run_js"))?
                    .call1((
                        "function isInstanceOf(object, constructor){ return (object instanceof \
                         constructor); } isInstanceOf",
                    ))?
                    .unbind())
            })
            .map(|x| x.bind(py))
    }

    is_instance_of(object.py())?
        .call1((object, constructor))?
        .extract()
}

pub fn create_js_object(py: Python) -> Result<Bound<PyAny>, PyErr> {
    fn js_object_new(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
        static JS_OBJECT_NEW: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        JS_OBJECT_NEW.import(py, "js.Object", "new")
    }

    js_object_new(py)?.call0()
}

pub fn py_to_js_proxy<T: PyTypeInfo>(object: Bound<T>) -> Result<Bound<PyAny>, PyErr> {
    fn to_js(py: Python<'_>) -> Result<&Bound<'_, PyAny>, PyErr> {
        static TO_JS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        TO_JS.import(py, "pyodide.ffi", "to_js")
    }

    let py = object.py();

    to_js(py)?.call(
        (object,),
        Some(&[(intern!(py, "create_pyproxies"), true)].into_py_dict(py)?),
    )
}

use pyo3::{intern, prelude::*};
use wasm_runtime_layer::{
    backend::{Extern, Value},
    ValueType,
};

use crate::Engine;

/// Converts a Rust type to Python
pub trait ToPy {
    /// Convert this value to Python
    fn to_py(&self, py: Python) -> Py<PyAny>;
}

impl ToPy for Value<Engine> {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        match self {
            Value::I32(v) => v.to_object(py),
            Value::I64(v) => v.to_object(py),
            Value::F32(v) => v.to_object(py),
            Value::F64(v) => v.to_object(py),
            Value::FuncRef(Some(func)) => func.to_py(py),
            Value::FuncRef(None) => py.None(),
            Value::ExternRef(Some(r#ref)) => r#ref.to_py(py),
            Value::ExternRef(None) => py.None(),
        }
    }
}

impl ToPy for Extern<Engine> {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        match self {
            Extern::Global(v) => v.to_py(py),
            Extern::Table(v) => v.to_py(py),
            Extern::Memory(v) => v.to_py(py),
            Extern::Func(v) => v.to_py(py),
        }
    }
}

pub trait ValueExt: Sized {
    /// Convert a value to its type
    fn ty(&self) -> ValueType;

    /// Convert the [`PyAny`] value into a Value of the supplied type
    fn from_py_typed(value: &PyAny, ty: &ValueType) -> anyhow::Result<Self>;
}

impl ValueExt for Value<Engine> {
    /// Convert a value to its type
    fn ty(&self) -> ValueType {
        match self {
            Value::I32(_) => ValueType::I32,
            Value::I64(_) => ValueType::I64,
            Value::F32(_) => ValueType::F32,
            Value::F64(_) => ValueType::F64,
            Value::FuncRef(_) => ValueType::FuncRef,
            Value::ExternRef(_) => ValueType::ExternRef,
        }
    }

    fn from_py_typed(value: &PyAny, ty: &ValueType) -> anyhow::Result<Self> {
        match ty {
            ValueType::I32 => Ok(Value::I32(value.extract()?)),
            ValueType::I64 => Ok(Value::I64(value.extract()?)),
            ValueType::F32 => Ok(Value::F32(value.extract()?)),
            ValueType::F64 => Ok(Value::F64(value.extract()?)),
            ValueType::FuncRef | ValueType::ExternRef => {
                anyhow::bail!(
                    "conversion to a function or extern outside of a module not permitted"
                )
            }
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

/// Check if `object` is an instance of the JavaScript class with `constructor`.
pub fn instanceof(py: Python, object: &PyAny, constructor: &PyAny) -> Result<bool, PyErr> {
    let instanceof = py
        .import(intern!(py, "pyodide"))?
        .getattr(intern!(py, "code"))?
        .getattr(intern!(py, "run_js"))?
        .call1((
            "function isInstanceOf(object, constructor){ return (object instanceof \
             constructor); } isInstanceOf",
        ))?;

    instanceof.call1((object, constructor))?.extract()
}

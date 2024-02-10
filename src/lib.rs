#![deny(warnings)]
#![warn(missing_docs)]

//! `pyodide_wasm_runtime_layer` implements the `wasm_runtime_layer` API to
//! provide access to the web browser's `WebAssembly` runtime using `pyodide`.

use std::{error::Error, fmt::Display};

use js_sys::{JsString, Reflect};
use pyo3::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_runtime_layer::{
    backend::{AsContextMut, Extern, Value, WasmEngine},
    ValueType,
};

/// Conversion to and from Python
mod conversion;
/// Extern host references
mod externref;
/// Functions
mod func;
/// Globals
mod global;
/// Instances
mod instance;
/// Memories
mod memory;
/// WebAssembly modules
mod module;
/// Stores all the WebAssembly state for a given collection of modules with a similar lifetime
mod store;
/// WebAssembly tables
mod table;

pub use externref::ExternRef;
pub use func::Func;
pub use global::Global;
pub use instance::Instance;
pub use memory::Memory;
pub use module::Module;
pub use store::{Store, StoreContext, StoreContextMut, StoreInner};
pub use table::Table;

use self::conversion::{FromJs, FromStoredJs, ToJs, ToPy, ToStoredJs};

/// Helper to convert a `JsValue` into a proper error, as well as making it `Send` + `Sync`
#[derive(Debug, Clone)]
pub(crate) struct JsErrorMsg {
    /// A string representation of the error message
    message: String,
}

impl Display for JsErrorMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

impl Error for JsErrorMsg {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

impl From<&JsValue> for JsErrorMsg {
    fn from(value: &JsValue) -> Self {
        if let Some(v) = value.dyn_ref::<JsString>() {
            Self { message: v.into() }
        } else if let Ok(v) = Reflect::get(value, &"message".into()) {
            Self {
                message: v.as_string().expect("A string object"),
            }
        } else {
            Self {
                message: format!("{value:?}"),
            }
        }
    }
}

impl From<JsValue> for JsErrorMsg {
    fn from(value: JsValue) -> Self {
        Self::from(&value)
    }
}

impl WasmEngine for Engine {
    type ExternRef = ExternRef;
    type Func = Func;
    type Global = Global;
    type Instance = Instance;
    type Memory = Memory;
    type Module = Module;
    type Store<T> = Store<T>;
    type StoreContext<'a, T: 'a> = StoreContext<'a, T>;
    type StoreContextMut<'a, T: 'a> = StoreContextMut<'a, T>;
    type Table = Table;
}

#[derive(Default, Debug, Clone)]
/// Runtime for WebAssembly
pub struct Engine {
    _private: (),
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
            Value::ExternRef(_) => todo!(),
        }
    }
}

impl FromStoredJs for Value<Engine> {
    /// Convert from a JavaScript value.
    ///
    /// Returns `None` if the value can not be represented
    fn from_stored_js<T>(store: &mut StoreInner<T>, value: JsValue) -> Option<Self> {
        let ty = &*value
            .js_typeof()
            .as_string()
            .expect("typeof returns a string");

        let res = match ty {
            "number" => Value::F64(f64::from_stored_js(store, value).unwrap()),
            "bigint" => Value::I64(i64::from_stored_js(store, value).unwrap()),
            "boolean" => Value::I32(bool::from_stored_js(store, value).unwrap() as i32),
            "null" => Value::I32(0),
            "function" => {
                #[cfg(feature = "tracing")]
                tracing::error!("conversion to a function outside of a module not permitted");
                return None;
            }
            // An instance of a WebAssembly.* class or null
            "object" => {
                if value.is_instance_of::<js_sys::Function>() {
                    #[cfg(feature = "tracing")]
                    tracing::error!("conversion to a function outside of a module not permitted");
                    return None;
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::error!(?value, "Unsupported value type");
                    return None;
                }
            }
            _ => {
                #[cfg(feature = "tracing")]
                tracing::error!(?ty, "Unknown value primitive type");
                return None;
            }
        };

        Some(res)
    }
}

impl ToStoredJs for Extern<Engine> {
    type Repr = JsValue;
    fn to_stored_js<T>(&self, _store: &StoreInner<T>) -> JsValue {
        match self {
            Extern::Global(_v) => todo!(), // FIXME
            Extern::Table(_v) => todo!(),  // FIXME
            Extern::Memory(_v) => todo!(), // FIXME
            Extern::Func(_v) => todo!(),   // FIXME
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

trait ValueTypeExt {
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

impl ToJs for ValueType {
    type Repr = JsString;
    /// Convert the value enum to a JavaScript descriptor
    ///
    /// See: <https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Global/Global>
    fn to_js(&self) -> JsString {
        self.as_js_descriptor().into()
    }
}

impl FromJs for ValueType {
    fn from_js(value: JsValue) -> Option<Self>
    where
        Self: Sized,
    {
        let s = value.as_string()?;

        let res = match &s[..] {
            "i32" => Self::I32,
            "i64" => Self::I64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            "anyfunc" => Self::FuncRef,
            "externref" => Self::ExternRef,
            _ => {
                #[cfg(feature = "tracing")]
                tracing::error!("Invalid value type {s:?}");
                return None;
            }
        };

        Some(res)
    }
}

trait ValueExt: Sized {
    /// Convert the JsValue into a Value of the supplied type
    fn from_js_typed<T>(store: &mut StoreInner<T>, ty: &ValueType, value: JsValue) -> Option<Self>;

    /// Convert the PyAny value into a Value of the supplied type
    fn from_py_typed<T>(
        store: &mut StoreInner<T>,
        ty: &ValueType,
        value: &PyAny,
    ) -> anyhow::Result<Self>;

    /// Convert a value to its type
    fn ty(&self) -> ValueType;
}

impl ValueExt for Value<Engine> {
    fn from_js_typed<T>(
        _store: &mut StoreInner<T>,
        ty: &ValueType,
        value: JsValue,
    ) -> Option<Self> {
        match ty {
            ValueType::I32 => Some(Value::I32(i32::from_js(value)?)),
            ValueType::I64 => Some(Value::I64(i64::from_js(value)?)),
            ValueType::F32 => Some(Value::F32(f32::from_js(value)?)),
            ValueType::F64 => Some(Value::F64(f64::from_js(value)?)),
            ValueType::FuncRef | ValueType::ExternRef => {
                #[cfg(feature = "tracing")]
                tracing::error!(
                    "conversion to a function or extern outside of a module not permitted"
                );
                None
            }
        }
    }

    fn from_py_typed<T>(
        _store: &mut StoreInner<T>,
        ty: &ValueType,
        value: &PyAny,
    ) -> anyhow::Result<Self> {
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
}

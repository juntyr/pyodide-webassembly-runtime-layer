use std::{any::TypeId, marker::PhantomData};

use pyo3::{
    intern,
    prelude::*,
    types::{IntoPyDict, PyTuple},
    PyTypeInfo,
};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmFunc, WasmStoreContext},
    FuncType,
};

use crate::{
    conversion::{ToPy, ValueExt},
    Engine, StoreContextMut,
};

/// A bound function
#[derive(Debug, Clone)]
pub struct Func {
    /// The inner function
    func: Py<PyAny>,
    /// The function signature
    ty: FuncType,
    user_state: Option<TypeId>,
}

impl WasmFunc<Engine> for Func {
    fn new<T>(
        mut ctx: impl AsContextMut<Engine, UserState = T>,
        ty: FuncType,
        func: impl 'static
            + Send
            + Sync
            + Fn(StoreContextMut<T>, &[Value<Engine>], &mut [Value<Engine>]) -> anyhow::Result<()>,
    ) -> Self {
        Python::with_gil(|py| -> Result<Self, PyErr> {
            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("Func::new").entered();

            let store: StoreContextMut<T> = ctx.as_context_mut();

            let user_state = non_static_type_id(store.data());
            let ty_clone = ty.clone();

            let func = Box::new(move |args: &PyTuple, ctx: &mut PyStoreContextMut| {
                assert_eq!(ctx.user_state, user_state);

                // Safety:
                //  - type casting: we just checked the type id
                //  - mutable reference:
                //    - PyStoreContextMut::ptr is constructed from a mutable
                //      reference
                //    - we ensure that PyStoreContextMut is only accessed for
                //      the lifetime of that mutable borrow
                let store: &mut StoreContextMut<T> = unsafe { &mut *ctx.ptr.cast() };

                let py = args.py();
                let ty = &ty_clone;

                let args = ty
                    .params()
                    .iter()
                    .zip(args.iter())
                    .map(|(ty, arg)| Value::from_py_typed(arg, ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut results = vec![Value::I32(0); ty.results().len()];

                #[cfg(feature = "tracing")]
                let _span = tracing::debug_span!("call_host", ?args, ?ty).entered();

                match func(store.as_context_mut(), &args, &mut results) {
                    Ok(()) => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(?results, "result");
                    }
                    Err(err) => {
                        #[cfg(feature = "tracing")]
                        tracing::error!("{err:?}");
                        return Err(err.into());
                    }
                }

                let results = PyTuple::new(py, results.into_iter().map(|res| res.to_py(py)));

                Ok(results.into_py(py))
            });

            let func = Py::new(py, PyFunc { func })?;

            Ok(Self {
                func: func.into_ref(py).into_py(py),
                ty,
                user_state: Some(user_state),
            })
        })
        .unwrap()
    }

    fn ty(&self, _ctx: impl AsContext<Engine>) -> FuncType {
        self.ty.clone()
    }

    fn call<T>(
        &self,
        mut ctx: impl AsContextMut<Engine>,
        args: &[Value<Engine>],
        results: &mut [Value<Engine>],
    ) -> anyhow::Result<()> {
        Python::with_gil(|py| {
            let mut store: StoreContextMut<_> = ctx.as_context_mut();

            let func = self.func.as_ref(py);

            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("call_guest", ?args, ?self.ty).entered();

            // https://webassembly.github.io/spec/js-api/#exported-function-exotic-objects
            assert_eq!(self.ty.params().len(), args.len());
            assert_eq!(self.ty.results().len(), results.len());

            // call may be to a WebAssembly function, so args must be turned into JS
            let args = args
                .iter()
                .map(|arg| arg.to_py_js(py))
                .collect::<Result<Vec<_>, _>>()?;
            let args = PyTuple::new(py, args);

            let kwargs = match self.user_state {
                None => None,
                Some(user_state) => {
                    assert_eq!(user_state, non_static_type_id(store.data()));

                    let store = PyStoreContextMut {
                        ptr: std::ptr::from_mut(&mut store).cast(),
                        user_state,
                    };

                    Some([(intern!(py, "ctx"), Py::new(py, store)?)].into_py_dict(py))
                }
            };

            let res = func.call(args, kwargs)?;

            #[cfg(feature = "tracing")]
            tracing::debug!(res, ?self.ty);

            match (self.ty.results(), results) {
                ([], []) => (),
                ([ty], [result]) => *result = Value::from_py_typed(res, ty)?,
                (tys, results) => {
                    let res: &PyTuple = PyTuple::type_object(py).call1((res,))?.extract()?;

                    // https://webassembly.github.io/spec/js-api/#exported-function-exotic-objects
                    assert_eq!(tys.len(), res.len());

                    for ((ty, result), value) in self
                        .ty
                        .results()
                        .iter()
                        .zip(results.iter_mut())
                        .zip(res.iter())
                    {
                        *result = Value::from_py_typed(value, ty)?;
                    }
                }
            }

            Ok(())
        })
    }
}

impl ToPy for Func {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        self.func.clone_ref(py)
    }
}

impl Func {
    /// Creates a new function from a Python value
    pub(crate) fn from_exported_function(
        value: &PyAny,
        signature: FuncType,
    ) -> anyhow::Result<Self> {
        let py = value.py();

        if !value.is_callable() {
            anyhow::bail!(
                "expected WebAssembly.Function but found {value:?} which is not callable"
            );
        }

        Ok(Self {
            func: value.into_py(py),
            ty: signature,
            user_state: None,
        })
    }
}

#[pyclass]
struct PyStoreContextMut {
    ptr: *mut StoreContextMut<'static, ()>,
    user_state: TypeId,
}

unsafe impl Send for PyStoreContextMut {}
unsafe impl Sync for PyStoreContextMut {}

#[pyclass(frozen)]
struct PyFunc {
    #[allow(clippy::type_complexity)]
    func: Box<
        dyn 'static
            + Send
            + Sync
            + Fn(&PyTuple, &mut PyStoreContextMut) -> Result<Py<PyTuple>, PyErr>,
    >,
}

#[pymethods]
impl PyFunc {
    #[pyo3(signature = (*args, ctx))]
    fn __call__(&self, args: &PyTuple, ctx: &mut PyStoreContextMut) -> Result<Py<PyTuple>, PyErr> {
        (self.func)(args, ctx)
    }
}

// Courtesy of David Tolnay:
// https://github.com/rust-lang/rust/issues/41875#issuecomment-317292888
fn non_static_type_id<T: ?Sized>(_x: &T) -> TypeId {
    trait NonStaticAny {
        fn get_type_id(&self) -> TypeId
        where
            Self: 'static;
    }

    impl<T: ?Sized> NonStaticAny for PhantomData<T> {
        fn get_type_id(&self) -> TypeId
        where
            Self: 'static,
        {
            TypeId::of::<T>()
        }
    }

    let phantom_data = PhantomData::<T>;
    NonStaticAny::get_type_id(unsafe {
        core::mem::transmute::<&dyn NonStaticAny, &(dyn NonStaticAny + 'static)>(&phantom_data)
    })
}

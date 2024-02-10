use std::{any::TypeId, marker::PhantomData};

use pyo3::{
    intern,
    prelude::*,
    types::{IntoPyDict, PyDict, PyTuple},
    PyTypeInfo,
};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmFunc, WasmStoreContext},
    FuncType,
};

use crate::{
    conversion::{py_to_js_proxy, ToPy, ValueExt},
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

            let _func = Box::new(move |args: &PyTuple, ctx: &mut PyStoreContextMut| -> Result<Py<PyAny>, PyErr> {
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

                let results = match results.as_slice() {
                    [] => py.None(),
                    [res] => res.to_py(py),
                    results => PyTuple::new(py, results.iter().map(|res| res.to_py(py)))
                        .as_ref()
                        .into_py(py),
                };

                Ok(results)
            });

            #[pyfunction]
            #[pyo3(signature = (*args))]
            fn dummy_callback(args: &PyTuple) {
                #[cfg(feature = "tracing")]
                let _span = tracing::trace_span!("dummy callback args", %args).entered();
                let _args = args;
            }

            let func = wrap_pyfunction!(dummy_callback, py)?;

            // let func = Py::new(
            //     py,
            //     PyFunc {
            //         _func: func,
            //         ty: ty.clone(),
            //     },
            // )?;

            Ok(Self {
                func: /*func.into_ref(py)*/func.into_py(py),
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
            tracing::debug!(%res, ?self.ty);

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
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("Func::to_py", %self.func, ?self.ty).entered();
        self.func.clone_ref(py)
    }

    fn to_py_js(&self, py: Python) -> Result<Py<PyAny>, PyErr> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("Func::to_py_js", %self.func, ?self.ty).entered();
        let func = py_to_js_proxy(py, self.func.as_ref(py))?;
        Ok(func.into_py(py))
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

        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("Func::from_exported_function", %value, ?signature).entered();

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
    _func: Box<
        dyn 'static
            + Send
            + Sync
            + Fn(&PyTuple, &mut PyStoreContextMut) -> Result<Py<PyAny>, PyErr>,
    >,
    ty: FuncType,
}

#[pymethods]
impl PyFunc {
    // #[pyo3(signature = (*args, ctx))]
    // fn __call__(&self, args: &PyTuple, ctx: &mut PyStoreContextMut) -> Result<Py<PyAny>, PyErr> {
    //     (self.func)(args, ctx)
    // }

    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(&self, args: &PyTuple, kwargs: Option<&PyDict>) -> Result<Py<PyAny>, PyErr> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("call_trampoline", ?self.ty).entered();

        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("args", %args).entered();

        #[cfg(feature = "tracing")]
        if let Some(kwargs) = kwargs {
            let _span = tracing::trace_span!("kwargs", %kwargs).entered();
        }

        Ok(args.py().None())
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

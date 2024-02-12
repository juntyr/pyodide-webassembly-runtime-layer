use std::{any::TypeId, marker::PhantomData, sync::Weak};

use pyo3::{prelude::*, types::PyTuple, PyTypeInfo};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmFunc, WasmStoreContext},
    FuncType,
};

use crate::{
    conversion::{py_to_js, py_to_js_proxy, py_to_weak_js, ToPy, ValueExt},
    Engine, StoreContextMut,
};

/// A bound function
#[derive(Debug, Clone)]
pub struct Func {
    /// The inner function
    func: Py<PyAny>,
    /// The function signature
    ty: FuncType,
    /// The user state type of the context
    user_state: Option<TypeId>,
}

impl Drop for Func {
    fn drop(&mut self) {
        Python::with_gil(|py| {
            let _func = self.func.as_ref(py);
            #[cfg(feature = "tracing")]
            tracing::debug!(?self.ty, refcnt = _func.get_refcnt(), "Func::drop");
        })
    }
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
            tracing::debug!("Func::new");

            let mut store: StoreContextMut<T> = ctx.as_context_mut();

            let weak_store = store.as_weak_proof();

            let user_state = non_static_type_id(store.data());
            let ty_clone = ty.clone();

            let func = Box::new(move |args: &PyTuple| -> Result<Py<PyAny>, PyErr> {
                let mut strong_store = Weak::upgrade(&weak_store)
                    .expect("host func must only be called while its store is alive");

                // Safety:
                //
                // - The proof is constructed from a mutable store context
                // - Calling a host function (from the host or from WASM)
                //   provides that call with a mutable reborrow of the
                //   store context
                let store = unsafe { StoreContextMut::from_proof_unchecked(&mut strong_store) };

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

                match func(store, &args, &mut results) {
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

            let func = Py::new(
                py,
                PyFunc {
                    func,
                    _ty: ty.clone(),
                },
            )?;
            let func = py_to_js_proxy(py, func.into_ref(py))?.into_py(py);

            Ok(Self {
                func,
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
            let store: StoreContextMut<_> = ctx.as_context_mut();

            if let Some(user_state) = self.user_state {
                assert_eq!(user_state, non_static_type_id(store.data()));
            }

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

            let res = func.call1(args)?;

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
        tracing::trace!(func = %self.func, ?self.ty, "Func::to_py");

        self.func.clone_ref(py)
    }

    // fn to_py_js(&self, py: Python) -> Result<Py<PyAny>, PyErr> {
    //     #[cfg(feature = "tracing")]
    //     tracing::trace!(func = %self.func, ?self.ty, "Func::to_py_js");

    //     let func = py_to_js_proxy(py, self.func.as_ref(py))?;
    //     Ok(func.into_py(py))
    // }
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
        tracing::debug!(%value, ?signature, "Func::from_exported_function");

        Ok(Self {
            func: py_to_weak_js(py, value)?.into_py(py),
            ty: signature,
            user_state: None,
        })
    }
}

#[pyclass(frozen)]
struct PyFunc {
    #[allow(clippy::type_complexity)]
    func: Box<dyn 'static + Send + Sync + Fn(&PyTuple) -> Result<Py<PyAny>, PyErr>>,
    _ty: FuncType,
}

#[pymethods]
impl PyFunc {
    #[pyo3(signature = (*args))]
    fn __call__(&self, py: Python, args: &PyTuple) -> Result<Py<PyAny>, PyErr> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("call_trampoline", ?self._ty, %args).entered();

        let result = (self.func)(args)?;
        let result = py_to_js(py, result.into_ref(py))?.into_py(py);

        Ok(result)
    }
}

impl Drop for PyFunc {
    fn drop(&mut self) {
        #[cfg(feature = "tracing")]
        tracing::debug!(?self._ty, "PyFunc::drop");
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

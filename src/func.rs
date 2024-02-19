use std::{
    any::TypeId,
    marker::PhantomData,
    sync::{Arc, OnceLock, Weak},
};

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
#[derive(Clone, Debug)]
pub struct Func {
    /// The inner function
    func: Py<PyAny>,
    // func: GuestOrHostFunc,
    /// The function signature
    ty: FuncType,
    /// The user state type of the context
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
            tracing::debug!("Func::new");

            let mut store: StoreContextMut<T> = ctx.as_context_mut();

            let weak_store = store.as_weak_proof();

            let user_state = non_static_type_id(store.data());
            let ty_clone = ty.clone();

            let func = Arc::new(
                move |py: Python, args: Py<PyTuple>| -> Result<Py<PyAny>, PyErr> {
                    let Some(mut strong_store) = Weak::upgrade(&weak_store) else {
                        return Err(PyErr::from(anyhow::anyhow!(
                            "host func called after free of its associated store"
                        )));
                    };

                    // Safety:
                    //
                    // - The proof is constructed from a mutable store context
                    // - Calling a host function (from the host or from WASM)
                    //   provides that call with a mutable reborrow of the
                    //   store context
                    let store = unsafe { StoreContextMut::from_proof_unchecked(&mut strong_store) };

                    let ty = &ty_clone;

                    let args = ty
                        .params()
                        .iter()
                        .zip(args.as_ref(py).iter())
                        .map(|(ty, arg)| Value::from_py_typed(py, arg.into_py(py), *ty))
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
                },
            );

            let func = Py::new(
                py,
                PyHostFunc {
                    func: store.register_host_func(func),
                    _ty: ty.clone(),
                },
            )?;
            let func = py_to_js_proxy(py, func)?;

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

            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("call_guest", ?args, ?self.ty).entered();

            // https://webassembly.github.io/spec/js-api/#exported-function-exotic-objects
            assert_eq!(self.ty.params().len(), args.len());
            assert_eq!(self.ty.results().len(), results.len());

            let args = args.iter().map(|arg| arg.to_py(py));
            let args = PyTuple::new(py, args);

            let res = self.func.call1(py, args)?;

            #[cfg(feature = "tracing")]
            tracing::debug!(%res, ?self.ty);

            match (self.ty.results(), results) {
                ([], []) => (),
                ([ty], [result]) => *result = Value::from_py_typed(py, res, *ty)?,
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
                        *result = Value::from_py_typed(py, value.into_py(py), *ty)?;
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
        py: Python,
        func: Py<PyAny>,
        ty: FuncType,
    ) -> anyhow::Result<Self> {
        if !func.as_ref(py).is_callable() {
            anyhow::bail!("expected WebAssembly.Function but found {func:?} which is not callable");
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(%func, ?ty, "Func::from_exported_function");

        Ok(Self {
            func,
            ty,
            user_state: None,
        })
    }
}

pub type PyHostFuncFn =
    dyn 'static + Send + Sync + Fn(Python, Py<PyTuple>) -> Result<Py<PyAny>, PyErr>;

#[pyclass(frozen)]
struct PyHostFunc {
    func: Weak<PyHostFuncFn>,
    _ty: FuncType,
}

#[pymethods]
impl PyHostFunc {
    #[pyo3(signature = (*args))]
    fn __call__(&self, py: Python, args: Py<PyTuple>) -> Result<Py<PyAny>, PyErr> {
        #[cfg(feature = "tracing")]
        let _span =
            tracing::debug_span!("call_trampoline", ?self._ty, args = %args.as_ref(py)).entered();

        let Some(func) = self.func.upgrade() else {
            return Err(PyErr::from(anyhow::anyhow!(
                "weak host func called after free of its associated store"
            )));
        };

        func(py, args)
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

fn py_to_js_proxy(py: Python, object: Py<PyHostFunc>) -> Result<Py<PyAny>, PyErr> {
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

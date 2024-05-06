use std::{
    fmt,
    marker::PhantomData,
    sync::{Arc, Weak},
};

use wasm_runtime_layer::backend::{
    AsContext, AsContextMut, WasmStore, WasmStoreContext, WasmStoreContextMut,
};
use wobbly::sync::Wobbly;

use crate::{func::PyHostFuncFn, Engine};

/// A store for the [`Engine`], which stores host-defined data `T` and internal
/// state.
pub struct Store<T> {
    /// The internal store is kept behind a pointer.
    ///
    /// This is to allow referencing and reconstructing a calling context in
    /// exported functions, where it is not possible to prove the correct
    /// lifetime and borrowing rules statically nor dynamically using
    /// `RefCell`s. This is because functions can be re-entrant with exclusive but
    /// stacked calling contexts. [`std::cell::RefCell`] and
    /// [`std::cell::RefMut`] do not allow for recursive usage by design
    /// (and it would be nigh impossible and quite expensive to enforce at
    /// runtime).
    ///
    /// The store is stored through a raw pointer, as using a `Pin<Box<T>>`
    /// would not be possible, despite the memory location of the Box
    /// contents technically being pinned in memory. This is because of the
    /// stacked borrows model.
    ///
    /// When the outer box is moved, it invalidates all tags in its borrow
    /// stack, even though the memory location remains. This invalidates all
    /// references and raw pointers to `T` created from the Box.
    ///
    /// See: <https://blog.nilstrieb.dev/posts/box-is-a-unique-type/> for more details.
    ///
    /// By using a box here, we would leave invalid pointers with revoked access
    /// permissions to the memory location of `T`.
    ///
    /// This creates undefined behavior as the Rust compiler will incorrectly
    /// optimize register accesses and memory loading and incorrect no-alias
    /// attributes.
    ///
    /// To circumvent this we can use a raw pointer obtained from unwrapping a
    /// Box.
    ///
    /// # Playground
    ///
    /// - `Pin<Box<T>>` solution (UB): <https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=685c984584bc0ca1faa780ca292f406c>
    /// - raw pointer solution (sound): <https://play.rust-lang.org/?version=stable&mode=release&edition=2021&gist=257841cb1675106d55c756ad59fde2fb>
    ///
    /// You can use `Tools > Miri` to test the validity
    inner: Arc<StoreProof>,
    /// Marker to strongly type the [`StoreProof`]
    _marker: PhantomData<T>,
}

/// The inner state of the store, which is pinned in heap memory
struct StoreInner<T> {
    /// The engine used
    engine: Engine,
    /// The user data
    data: T,
    /// The user host functions, which must live in Rust and not JS to avoid a
    /// cross-language reference cycle
    host_funcs: Vec<Wobbly<PyHostFuncFn>>,
}

impl<T> WasmStore<T, Engine> for Store<T> {
    fn new(engine: &Engine, data: T) -> Self {
        #[cfg(feature = "tracing")]
        tracing::debug!("Store::new");

        Self {
            inner: Arc::new(StoreProof::from_ptr(Box::into_raw(Box::new(StoreInner {
                engine: engine.clone(),
                data,
                host_funcs: Vec::new(),
            })))),
            _marker: PhantomData::<T>,
        }
    }

    fn engine(&self) -> &Engine {
        &self.as_inner().engine
    }

    fn data(&self) -> &T {
        &self.as_inner().data
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.as_inner_mut().data
    }

    fn into_data(self) -> T {
        let this = std::mem::ManuallyDrop::new(self);

        // Safety:
        //
        // This is the only read from self, which will not be dropped, so no duplication
        // occurs
        let inner = Arc::into_inner(unsafe { std::ptr::read(&this.inner) })
            .expect("Store owns the only strong reference to StoreInner");

        // Safety:
        //
        // Ownership of `self` signifies that no guest stack is currently active
        let inner = unsafe { Box::from_raw(inner.as_ptr()) };
        inner.data
    }
}

impl<T> AsContext<Engine> for Store<T> {
    type UserState = T;

    fn as_context(&self) -> StoreContext<'_, T> {
        StoreContext {
            store: self.as_inner(),
            proof: &self.inner,
        }
    }
}

impl<T> AsContextMut<Engine> for Store<T> {
    fn as_context_mut(&mut self) -> StoreContextMut<'_, T> {
        // Safety:
        //
        // A mutable reference to the store signifies mutable ownership, and is thus
        // safe.
        let store = unsafe { &mut *self.inner.as_ptr() };

        StoreContextMut {
            store,
            proof: &mut self.inner,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Store<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let store = self.as_inner();

        fmt.debug_struct("Store")
            .field("engine", &store.engine)
            .field("data", &store.data)
            .finish_non_exhaustive()
    }
}

impl<T: Default> Default for Store<T> {
    fn default() -> Self {
        Self::new(&Engine::default(), T::default())
    }
}

impl<T: Clone> Clone for Store<T> {
    fn clone(&self) -> Self {
        Self::new(self.engine(), self.data().clone())
    }
}

impl<T> Drop for Store<T> {
    fn drop(&mut self) {
        std::mem::drop(unsafe { Box::from_raw(self.inner.as_ptr::<T>()) });

        #[cfg(feature = "tracing")]
        tracing::debug!("Store::drop");
    }
}

impl<T> Store<T> {
    fn as_inner(&self) -> &StoreInner<T> {
        // Safety:
        //
        // A shared reference to the store signifies a non-mutable ownership, and is
        // thus safe.
        unsafe { &*self.inner.as_ptr() }
    }

    fn as_inner_mut(&mut self) -> &mut StoreInner<T> {
        // Safety:
        //
        // A mutable reference to the store signifies mutable ownership, and is thus
        // safe.
        unsafe { &mut *self.inner.as_ptr() }
    }
}

#[allow(clippy::module_name_repetitions)]
/// Immutable context to the store
pub struct StoreContext<'a, T: 'a> {
    /// The store
    store: &'a StoreInner<T>,
    /// Proof that the store is being kept alive
    proof: &'a Arc<StoreProof>,
}

#[allow(clippy::module_name_repetitions)]
/// Mutable context to the store
pub struct StoreContextMut<'a, T: 'a> {
    /// The store
    store: &'a mut StoreInner<T>,
    /// Proof that the store is being kept alive
    proof: &'a mut Arc<StoreProof>,
}

impl<'a, T: 'a> StoreContextMut<'a, T> {
    /// Returns a weak proof for having a mutable borrow of the inner store
    ///
    /// Since the inner store provides no API surface, this weak pointer can
    /// only be used with [`Self::from_proof_unchecked`].
    pub(crate) fn as_weak_proof(&mut self) -> Weak<StoreProof> {
        Arc::downgrade(self.proof)
    }

    /// Reconstructs the [`StoreContextMut`] from a strong proof of having
    /// a mutable borrow of the inner store.
    ///
    /// # Safety
    ///
    /// The `proof` must have been constructed from a [`StoreContextMut`] with
    /// the same generic type `T` as the one that is now created.
    ///
    /// The caller must be allowed to obtain a mutable (re-)borrow to the inner
    /// store for the lifetime `'a`.
    pub(crate) unsafe fn from_proof_unchecked(proof: &'a mut Arc<StoreProof>) -> Self {
        Self {
            store: unsafe { &mut *(proof.as_ptr()) },
            proof,
        }
    }

    pub(crate) fn register_host_func(&mut self, func: Arc<PyHostFuncFn>) -> Wobbly<PyHostFuncFn> {
        let func = Wobbly::new(func);
        self.store.host_funcs.push(func.clone());
        func
    }
}

impl<'a, T: 'a> WasmStoreContext<'a, T, Engine> for StoreContext<'a, T> {
    fn engine(&self) -> &Engine {
        &self.store.engine
    }

    fn data(&self) -> &T {
        &self.store.data
    }
}

impl<'a, T: 'a> AsContext<Engine> for StoreContext<'a, T> {
    type UserState = T;

    fn as_context(&self) -> StoreContext<'_, T> {
        StoreContext {
            store: self.store,
            proof: self.proof,
        }
    }
}

impl<'a, T: 'a> WasmStoreContext<'a, T, Engine> for StoreContextMut<'a, T> {
    fn engine(&self) -> &Engine {
        &self.store.engine
    }

    fn data(&self) -> &T {
        &self.store.data
    }
}

impl<'a, T: 'a> WasmStoreContextMut<'a, T, Engine> for StoreContextMut<'a, T> {
    fn data_mut(&mut self) -> &mut T {
        &mut self.store.data
    }
}

impl<'a, T: 'a> AsContext<Engine> for StoreContextMut<'a, T> {
    type UserState = T;

    fn as_context(&self) -> StoreContext<'_, T> {
        StoreContext {
            store: self.store,
            proof: self.proof,
        }
    }
}

impl<'a, T: 'a> AsContextMut<Engine> for StoreContextMut<'a, T> {
    fn as_context_mut(&mut self) -> StoreContextMut<'_, T> {
        StoreContextMut {
            store: self.store,
            proof: self.proof,
        }
    }
}

#[allow(clippy::module_name_repetitions)]
/// Helper type to transfer an opaque pointer to a [`StoreInner`]
pub struct StoreProof(*mut ());

unsafe impl Send for StoreProof {}
unsafe impl Sync for StoreProof {}

impl StoreProof {
    const fn from_ptr<T>(ptr: *mut StoreInner<T>) -> Self {
        Self(ptr.cast())
    }

    const fn as_ptr<T>(&self) -> *mut StoreInner<T> {
        self.0.cast()
    }
}

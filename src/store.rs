use wasm_runtime_layer::backend::{
    AsContext, AsContextMut, WasmStore, WasmStoreContext, WasmStoreContextMut,
};

use crate::Engine;

#[derive(Debug, Default, Clone)]
/// A collection of WebAssembly instances and host-defined state
pub struct Store<T> {
    /// The engine used
    engine: Engine,
    /// The user data
    data: T,
}

impl<T> WasmStore<T, Engine> for Store<T> {
    fn new(engine: &Engine, data: T) -> Self {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("Store::new").entered();

        Self {
            engine: engine.clone(),
            data,
        }
    }

    fn engine(&self) -> &Engine {
        &self.engine
    }

    fn data(&self) -> &T {
        &self.data
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    fn into_data(self) -> T {
        self.data
    }
}

impl<T> AsContext<Engine> for Store<T> {
    type UserState = T;

    fn as_context(&self) -> StoreContext<'_, T> {
        StoreContext { store: self }
    }
}

impl<T> AsContextMut<Engine> for Store<T> {
    fn as_context_mut(&mut self) -> StoreContextMut<'_, T> {
        StoreContextMut { store: self }
    }
}

/// Immutable context to the store
pub struct StoreContext<'a, T: 'a> {
    /// The store
    store: &'a Store<T>,
}

/// Mutable context to the store
pub struct StoreContextMut<'a, T: 'a> {
    /// The store
    store: &'a mut Store<T>,
}

impl<'a, T: 'a> WasmStoreContext<'a, T, Engine> for StoreContext<'a, T> {
    fn engine(&self) -> &Engine {
        self.store.engine()
    }

    fn data(&self) -> &T {
        self.store.data()
    }
}

impl<'a, T: 'a> AsContext<Engine> for StoreContext<'a, T> {
    type UserState = T;

    fn as_context(&self) -> StoreContext<'_, T> {
        StoreContext { store: self.store }
    }
}

impl<'a, T: 'a> WasmStoreContext<'a, T, Engine> for StoreContextMut<'a, T> {
    fn engine(&self) -> &Engine {
        self.store.engine()
    }

    fn data(&self) -> &T {
        self.store.data()
    }
}

impl<'a, T: 'a> WasmStoreContextMut<'a, T, Engine> for StoreContextMut<'a, T> {
    fn data_mut(&mut self) -> &mut T {
        self.store.data_mut()
    }
}

impl<'a, T: 'a> AsContext<Engine> for StoreContextMut<'a, T> {
    type UserState = T;

    fn as_context(&self) -> StoreContext<'_, T> {
        StoreContext { store: self.store }
    }
}

impl<'a, T: 'a> AsContextMut<Engine> for StoreContextMut<'a, T> {
    fn as_context_mut(&mut self) -> StoreContextMut<'_, T> {
        StoreContextMut { store: self.store }
    }
}

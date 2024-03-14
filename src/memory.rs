use std::sync::OnceLock;

use pyo3::{intern, prelude::*, types::PyBytes};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, WasmMemory},
    MemoryType,
};

use crate::{
    conversion::{create_js_object, instanceof, js_uint8_array, ToPy},
    Engine,
};

#[derive(Clone, Debug)]
/// A WASM memory.
///
/// This type wraps a [`WebAssembly.Memory`] from the JavaScript API.
///
/// [`WebAssembly.Memory`]: https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Memory
pub struct Memory {
    /// The memory value
    memory: Py<PyAny>,
    /// The memory type
    ty: MemoryType,
}

impl WasmMemory<Engine> for Memory {
    fn new(_ctx: impl AsContextMut<Engine>, ty: MemoryType) -> anyhow::Result<Self> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(?ty, "Memory::new");

            let desc = create_js_object(py)?;
            desc.setattr(py, intern!(py, "initial"), ty.initial_pages())?;
            if let Some(maximum) = ty.maximum_pages() {
                desc.setattr(py, intern!(py, "maximum"), maximum)?;
            }

            let memory = web_assembly_memory(py).call_method1(py, intern!(py, "new"), (desc,))?;

            Ok(Self { memory, ty })
        })
    }

    fn ty(&self, _ctx: impl AsContext<Engine>) -> MemoryType {
        self.ty
    }

    fn grow(&self, _ctx: impl AsContextMut<Engine>, additional: u32) -> anyhow::Result<u32> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %self.memory.as_ref(py), ?self.ty, additional, "Memory::grow");

            let old_pages = self
                .memory
                .call_method1(py, intern!(py, "grow"), (additional,))?
                .extract(py)?;

            Ok(old_pages)
        })
    }

    fn current_pages(&self, _ctx: impl AsContext<Engine>) -> u32 {
        const PAGE_SIZE: u64 = 1 << 16;

        Python::with_gil(|py| -> Result<u32, PyErr> {
            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %self.memory.as_ref(py), ?self.ty, "Memory::current_pages");

            let byte_len: u64 = self
                .memory
                .getattr(py, intern!(py, "buffer"))?
                .getattr(py, intern!(py, "byteLength"))?
                .extract(py)?;

            let pages = u32::try_from(byte_len / PAGE_SIZE)?;
            Ok(pages)
        })
        .unwrap()
    }

    fn read(
        &self,
        _ctx: impl AsContext<Engine>,
        offset: usize,
        buffer: &mut [u8],
    ) -> anyhow::Result<()> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %self.memory.as_ref(py), ?self.ty, offset, len = buffer.len(), "Memory::read");

            let memory = self.memory.getattr(py, intern!(py, "buffer"))?;
            let memory = js_uint8_array(py).call_method1(
                py,
                intern!(py, "new"),
                (memory, offset, buffer.len()),
            )?;

            let bytes: Py<PyBytes> = memory
                .call_method0(py, intern!(py, "to_bytes"))?
                .extract(py)?;
            buffer.copy_from_slice(bytes.as_ref(py).as_bytes());

            Ok(())
        })
    }

    fn write(
        &self,
        _ctx: impl AsContextMut<Engine>,
        offset: usize,
        buffer: &[u8],
    ) -> anyhow::Result<()> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %self.memory.as_ref(py), ?self.ty, offset, len = buffer.len(), "Memory::write");

            let memory = self.memory.getattr(py, intern!(py, "buffer"))?;
            let memory = js_uint8_array(py).call_method1(
                py,
                intern!(py, "new"),
                (memory, offset, buffer.len()),
            )?;

            memory.call_method1(py, intern!(py, "assign"), (buffer,))?;

            Ok(())
        })
    }
}

impl ToPy for Memory {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(value = %self.memory.as_ref(py), ?self.ty, "Memory::to_py");

        self.memory.clone_ref(py)
    }
}

impl Memory {
    /// Construct a memory from an exported memory object
    pub(crate) fn from_exported_memory(
        py: Python,
        memory: Py<PyAny>,
        ty: MemoryType,
    ) -> anyhow::Result<Self> {
        if !instanceof(py, &memory, web_assembly_memory(py))? {
            anyhow::bail!(
                "expected WebAssembly.Memory but found {}",
                memory.as_ref(py)
            );
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(memory = %memory.as_ref(py), ?ty, "Memory::from_exported_memory");

        Ok(Self { memory, ty })
    }
}

fn web_assembly_memory(py: Python) -> &'static Py<PyAny> {
    static WEB_ASSEMBLY_MEMORY: OnceLock<Py<PyAny>> = OnceLock::new();
    // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
    WEB_ASSEMBLY_MEMORY.get_or_init(|| {
        py.import(intern!(py, "js"))
            .unwrap()
            .getattr(intern!(py, "WebAssembly"))
            .unwrap()
            .getattr(intern!(py, "Memory"))
            .unwrap()
            .into_py(py)
    })
}

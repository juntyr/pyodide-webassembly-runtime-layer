use pyo3::{intern, prelude::*, sync::GILOnceCell, types::PyBytes};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, WasmMemory},
    MemoryType,
};

use crate::{
    conversion::{create_js_object, instanceof, js_uint8_array_new, ToPy},
    Engine,
};

#[derive(Debug)]
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

impl Clone for Memory {
    fn clone(&self) -> Self {
        Python::with_gil(|py| Self {
            memory: self.memory.clone_ref(py),
            ty: self.ty,
        })
    }
}

impl WasmMemory<Engine> for Memory {
    fn new(_ctx: impl AsContextMut<Engine>, ty: MemoryType) -> anyhow::Result<Self> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(?ty, "Memory::new");

            let desc = create_js_object(py)?;
            desc.setattr(intern!(py, "initial"), ty.initial_pages())?;
            if let Some(maximum) = ty.maximum_pages() {
                desc.setattr(intern!(py, "maximum"), maximum)?;
            }

            let memory = web_assembly_memory(py)?.call_method1(intern!(py, "new"), (desc,))?;

            Ok(Self {
                memory: memory.unbind(),
                ty,
            })
        })
    }

    fn ty(&self, _ctx: impl AsContext<Engine>) -> MemoryType {
        self.ty
    }

    fn grow(&self, _ctx: impl AsContextMut<Engine>, additional: u32) -> anyhow::Result<u32> {
        Python::with_gil(|py| {
            let memory = self.memory.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %memory, ?self.ty, additional, "Memory::grow");

            let old_pages = memory
                .call_method1(intern!(py, "grow"), (additional,))?
                .extract()?;

            Ok(old_pages)
        })
    }

    fn current_pages(&self, _ctx: impl AsContext<Engine>) -> u32 {
        const PAGE_SIZE: u64 = 1 << 16;

        Python::with_gil(|py| -> Result<u32, PyErr> {
            let memory = self.memory.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %memory, ?self.ty, "Memory::current_pages");

            let byte_len: u64 = memory
                .getattr(intern!(py, "buffer"))?
                .getattr(intern!(py, "byteLength"))?
                .extract()?;

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
            let memory = self.memory.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %memory, ?self.ty, offset, len = buffer.len(), "Memory::read");

            let memory = memory.getattr(intern!(py, "buffer"))?;
            let memory = js_uint8_array_new(py)?.call1((memory, offset, buffer.len()))?;

            let bytes: Bound<PyBytes> = memory.call_method0(intern!(py, "to_bytes"))?.extract()?;
            buffer.copy_from_slice(bytes.as_bytes());

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
            let memory = self.memory.bind(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(memory = %memory, ?self.ty, offset, len = buffer.len(), "Memory::write");

            let memory = memory.getattr(intern!(py, "buffer"))?;
            let memory = js_uint8_array_new(py)?.call1((memory, offset, buffer.len()))?;

            memory.call_method1(intern!(py, "assign"), (buffer,))?;

            Ok(())
        })
    }
}

impl ToPy for Memory {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(value = %self.memory.bind(py), ?self.ty, "Memory::to_py");

        self.memory.clone_ref(py)
    }
}

impl Memory {
    /// Construct a memory from an exported memory object
    pub(crate) fn from_exported_memory(
        memory: Bound<PyAny>,
        ty: MemoryType,
    ) -> anyhow::Result<Self> {
        if !instanceof(&memory, web_assembly_memory(memory.py())?)? {
            anyhow::bail!("expected WebAssembly.Memory but found {memory}");
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(memory = %memory, ?ty, "Memory::from_exported_memory");

        Ok(Self {
            memory: memory.unbind(),
            ty,
        })
    }
}

fn web_assembly_memory(py: Python) -> Result<&Bound<PyAny>, PyErr> {
    static WEB_ASSEMBLY_MEMORY: GILOnceCell<Py<PyAny>> = GILOnceCell::new();
    WEB_ASSEMBLY_MEMORY.import(py, "js.WebAssembly", "Memory")
}

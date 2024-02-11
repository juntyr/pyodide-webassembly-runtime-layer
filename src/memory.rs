use pyo3::{
    intern,
    prelude::*,
    types::{PyBytes, PyDict},
};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, WasmMemory},
    MemoryType,
};

use crate::{
    conversion::{instanceof, py_dict_to_js_object, ToPy},
    Engine,
};

#[derive(Debug, Clone)]
/// A WebAssembly Memory
pub struct Memory {
    /// The memory value
    value: Py<PyAny>,
    /// The memory type
    ty: MemoryType,
}

impl Drop for Memory {
    fn drop(&mut self) {
        Python::with_gil(|py| {
            let memory = self.value.as_ref(py);
            let _res = memory.call_method0(intern!(py, "destroy"));
            #[cfg(feature = "tracing")]
            match _res {
                Ok(ok) => tracing::debug!(?self.ty, %ok, "Memory::drop"),
                Err(err) => tracing::debug!(?self.ty, %err, "Memory::drop"),
            }
        })
    }
}

impl WasmMemory<Engine> for Memory {
    fn new(_ctx: impl AsContextMut<Engine>, ty: MemoryType) -> anyhow::Result<Self> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            tracing::debug!(?ty, "Memory::new");

            let desc = PyDict::new(py);
            desc.set_item(intern!(py, "initial"), ty.initial_pages())?;
            if let Some(maximum) = ty.maximum_pages() {
                desc.set_item(intern!(py, "maximum"), maximum)?;
            }
            let desc = py_dict_to_js_object(py, desc)?;

            let memory = web_assembly_memory(py)?
                .getattr(intern!(py, "new"))?
                .call1((desc,))?;

            Ok(Self {
                ty,
                value: memory.into_py(py),
            })
        })
    }

    fn ty(&self, _ctx: impl AsContext<Engine>) -> MemoryType {
        self.ty
    }

    fn grow(&self, _ctx: impl AsContextMut<Engine>, additional: u32) -> anyhow::Result<u32> {
        Python::with_gil(|py| {
            let memory = self.value.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%memory, ?self.ty, additional, "Memory::grow");

            let old_pages = memory
                .call_method1(intern!(py, "grow"), (additional,))?
                .extract()?;

            Ok(old_pages)
        })
    }

    fn current_pages(&self, _ctx: impl AsContext<Engine>) -> u32 {
        const PAGE_SIZE: u64 = 1 << 16;

        Python::with_gil(|py| -> Result<u32, PyErr> {
            let memory = self.value.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%memory, ?self.ty, "Memory::current_pages");

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
            let memory = self.value.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%memory, ?self.ty, offset, len = buffer.len(), "Memory::read");

            let memory = memory.getattr(intern!(py, "buffer"))?;
            let memory = py
                .import(intern!(py, "js"))?
                .getattr(intern!(py, "Uint8Array"))?
                .call_method1(intern!(py, "new"), (memory, offset, buffer.len()))?;

            let bytes: &PyBytes = memory.call_method0(intern!(py, "to_bytes"))?.extract()?;
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
            let memory = self.value.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%memory, ?self.ty, offset, len = buffer.len(), "Memory::write");

            let memory = memory.getattr(intern!(py, "buffer"))?;
            let memory = py
                .import(intern!(py, "js"))?
                .getattr(intern!(py, "Uint8Array"))?
                .call_method1(intern!(py, "new"), (memory, offset, buffer.len()))?;

            memory.call_method1(intern!(py, "assign"), (buffer,))?;

            Ok(())
        })
    }
}

impl ToPy for Memory {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(value = %self.value, ?self.ty, "Memory::to_py");

        self.value.clone_ref(py)
    }
}

impl Memory {
    /// Construct a memory from an exported memory object
    pub(crate) fn from_exported_memory(value: &PyAny, ty: MemoryType) -> anyhow::Result<Self> {
        let py = value.py();

        if !instanceof(py, value, web_assembly_memory(py)?)? {
            anyhow::bail!("expected WebAssembly.Memory but found {value:?}");
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(%value, ?ty, "Memory::from_exported_memory");

        Ok(Self {
            value: value.into_py(py),
            ty,
        })
    }
}

fn web_assembly_memory(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Memory"))
}

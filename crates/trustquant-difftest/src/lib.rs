//! wasmi-based WASM differential runner (v1 pattern restored; ADR-0003:
//! WASM is a validation target, not a shipped interface).

use serde_json::Value;
use wasmi::{Engine, Linker, Module, Store};

pub struct WasmDispatcher {
    store: Store<()>,
    memory: wasmi::Memory,
    alloc: wasmi::Func,
    dispatch: wasmi::Func,
}

impl WasmDispatcher {
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Self::new(&bytes)
    }

    pub fn new(wasm_bytes: &[u8]) -> Result<Self, String> {
        let engine = Engine::default();
        let module = Module::new(&engine, wasm_bytes).map_err(|e| format!("compile: {e}"))?;
        let mut store = Store::new(&engine, ());
        let linker = <Linker<()>>::new(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("instantiate: {e}"))?
            .start(&mut store)
            .map_err(|e| format!("start: {e}"))?;
        Ok(WasmDispatcher {
            memory: instance.get_memory(&store, "memory").ok_or("no memory")?,
            alloc: instance.get_func(&store, "qt_alloc").ok_or("no qt_alloc")?,
            dispatch: instance
                .get_func(&store, "qt_dispatch")
                .ok_or("no qt_dispatch")?,
            store,
        })
    }

    pub fn dispatch(&mut self, request: &Value) -> Result<Value, String> {
        let payload = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        let len = payload.len() as i32;
        if len == 0 {
            return Err("empty request".into());
        }
        let mut out = [wasmi::Val::I32(0)];
        self.alloc
            .call(&mut self.store, &[wasmi::Val::I32(len)], &mut out)
            .map_err(|e| format!("qt_alloc: {e}"))?;
        let in_ptr = match out[0] {
            wasmi::Val::I32(p) => p as usize,
            _ => return Err("alloc type".into()),
        };
        self.memory.data_mut(&mut self.store)[in_ptr..in_ptr + payload.len()]
            .copy_from_slice(&payload);
        let mut out2 = [wasmi::Val::I32(0)];
        self.dispatch
            .call(
                &mut self.store,
                &[wasmi::Val::I32(in_ptr as i32), wasmi::Val::I32(len)],
                &mut out2,
            )
            .map_err(|e| format!("qt_dispatch: {e}"))?;
        let out_ptr = match out2[0] {
            wasmi::Val::I32(p) => p as usize,
            _ => return Err("dispatch type".into()),
        };
        let mem = self.memory.data(&self.store);
        let out_len = u64::from_le_bytes(mem[out_ptr..out_ptr + 8].try_into().unwrap()) as usize;
        serde_json::from_slice(&mem[out_ptr + 8..out_ptr + 8 + out_len])
            .map_err(|e| format!("response parse: {e}"))
    }
}

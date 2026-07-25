use wasmtime::{Config, Engine, Instance, Linker, Module, Store, Val};

pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    pub fn new() -> anyhow::Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(true);

        let engine = Engine::new(&config)?;
        Ok(Self { engine })
    }

    pub fn load_module(&self, bytes: &[u8]) -> anyhow::Result<WasmModule> {
        let module = Module::new(&self.engine, bytes)?;
        Ok(WasmModule { module })
    }

    fn engine(&self) -> &Engine {
        &self.engine
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().expect("WasmRuntime::default")
    }
}

pub struct WasmModule {
    module: Module,
}

impl WasmModule {
    pub fn instantiate(&self, runtime: &WasmRuntime) -> anyhow::Result<WasmInstance> {
        let linker = Linker::new(runtime.engine());
        let mut store = Store::new(runtime.engine(), ());
        store.set_fuel(1_000_000)?;
        let instance = linker.instantiate(&mut store, &self.module)?;
        Ok(WasmInstance { instance, store })
    }
}

pub struct WasmInstance {
    instance: Instance,
    store: Store<()>,
}

impl WasmInstance {
    pub fn call_func(&mut self, name: &str, params: &[Val]) -> anyhow::Result<Vec<Val>> {
        let func = self
            .instance
            .get_func(&mut self.store, name)
            .ok_or_else(|| anyhow::anyhow!("exported function '{}' not found", name))?;

        let ty = func.ty(&self.store);
        let results_count = ty.results().len();
        let mut results = vec![Val::I32(0); results_count];
        func.call(&mut self.store, params, &mut results)?;
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_and_call_add() {
        let runtime = WasmRuntime::new().unwrap();
        let wat = r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add
                )
            )
        "#;
        let module = runtime.load_module(wat.as_bytes()).unwrap();
        let mut instance = module.instantiate(&runtime).unwrap();

        let result = instance
            .call_func("add", &[Val::I32(2), Val::I32(3)])
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].i32(), Some(5));
    }
}

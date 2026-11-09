//! Sandboxed execution for extension modules (ADR 0001, first runner slice).
//!
//! This is a wasmi interpreter, not a JIT: there is no ambient filesystem,
//! network, environment, clock or randomness, because no host functions are
//! linked at all. A module must export `run() -> i32`, may not import anything,
//! and is bounded by a fuel budget and a memory/table limiter.
//!
//! The slice deliberately grants no capability. A manifest that requests one is
//! refused by [`crate::extensions::run`] before this module is reached, so the
//! only thing extension code can do today is compute and return an exit code.
//! Workspace host functions are the next gate in the ADR, not a silent addition
//! here.

use std::fs;
use std::path::Path;
use std::time::Instant;
use wasmi::errors::{MemoryError, TableError};
use wasmi::{Config, Engine, Linker, Module, ResourceLimiter, Store};

/// Enough for useful pure computation, small enough that a runaway loop stops in
/// well under a second on this interpreter.
const FUEL: u64 = 50_000_000;
const MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const MAX_TABLE_ELEMENTS: u32 = 100_000;
const MAX_MODULE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct Outcome {
    pub exit_code: i32,
    pub fuel_used: u64,
    pub elapsed_ms: u128,
}

struct Limits {
    memory_bytes: usize,
    table_elements: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_bytes: MAX_MEMORY_BYTES,
            table_elements: MAX_TABLE_ELEMENTS,
        }
    }
}

impl ResourceLimiter for Limits {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, MemoryError> {
        Ok(desired <= self.memory_bytes)
    }

    fn table_growing(
        &mut self,
        _current: u32,
        desired: u32,
        _maximum: Option<u32>,
    ) -> Result<bool, TableError> {
        Ok(desired <= self.table_elements)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        1
    }

    fn memories(&self) -> usize {
        1
    }
}

pub(crate) fn run_file(path: &Path) -> Result<Outcome, String> {
    let meta = fs::metadata(path).map_err(|error| format!("The module cannot be read: {error}"))?;
    if meta.len() > MAX_MODULE_BYTES {
        return Err(format!(
            "The module is larger than the {} MiB limit.",
            MAX_MODULE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = fs::read(path).map_err(|error| format!("The module cannot be read: {error}"))?;
    run_bytes(&bytes)
}

fn run_bytes(bytes: &[u8]) -> Result<Outcome, String> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &mut &bytes[..])
        .map_err(|error| format!("Not valid WebAssembly: {error}"))?;
    if module.imports().next().is_some() {
        return Err(
            "This module imports a host function; no host functions are available to extensions yet."
                .to_string(),
        );
    }
    let mut store = Store::new(&engine, Limits::default());
    store.limiter(|limits| limits);
    store
        .add_fuel(FUEL)
        .map_err(|error| format!("Fuel metering is unavailable: {error}"))?;
    let linker = Linker::new(&engine);
    let started = Instant::now();
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|error| format!("The module could not be instantiated: {error}"))?
        .start(&mut store)
        .map_err(|error| format!("The module could not start: {error}"))?;
    let run = instance
        .get_typed_func::<(), i32>(&store, "run")
        .map_err(|_| "The module must export run() -> i32.".to_string())?;
    let exit_code = run.call(&mut store, ()).map_err(|error| {
        // A trap leaves a little residual fuel, so the trap code is the reliable
        // signal — `fuel_consumed() == FUEL` does not hold exactly.
        let out_of_fuel =
            matches!(error.trap_code(), Some(wasmi::core::TrapCode::OutOfFuel));
        if out_of_fuel {
            format!("Stopped after using the {FUEL}-fuel budget.")
        } else {
            format!("The module trapped: {error}")
        }
    })?;
    Ok(Outcome {
        exit_code,
        fuel_used: store.fuel_consumed().unwrap_or(0),
        elapsed_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn module_file(name: &str, source: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "free-ai-studio-extension-run-{}-{name}.wasm",
            std::process::id()
        ));
        let bytes = wat::parse_str(source).expect("wat compiles");
        fs::write(&path, bytes).expect("module file");
        path
    }

    #[test]
    fn a_minimal_module_returns_its_exit_code() {
        let path = module_file(
            "ok",
            r#"(module (func (export "run") (result i32) i32.const 7))"#,
        );
        let outcome = run_file(&path).expect("run");
        assert_eq!(outcome.exit_code, 7);
        assert!(outcome.fuel_used > 0);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_endless_loop_is_stopped_by_the_fuel_budget() {
        let path = module_file(
            "spin",
            r#"(module
                 (func (export "run") (result i32)
                   (local $n i32)
                   (loop $spin
                     (local.set $n (i32.add (local.get $n) (i32.const 1)))
                     (br $spin))
                   (i32.const 0)))"#,
        );
        let error = run_file(&path).expect_err("fuel stops the loop");
        assert!(error.contains("fuel budget"), "{error}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn imports_and_a_missing_run_export_are_refused() {
        let imported = module_file(
            "imports",
            r#"(module
                 (import "host" "read" (func))
                 (func (export "run") (result i32) i32.const 0))"#,
        );
        let error = run_file(&imported).expect_err("imports refused");
        assert!(error.contains("imports a host function"), "{error}");
        let _ = fs::remove_file(&imported);

        let no_run = module_file(
            "no-run",
            r#"(module (func (export "other") (result i32) i32.const 0))"#,
        );
        let error = run_file(&no_run).expect_err("run export required");
        assert!(error.contains("must export run()"), "{error}");
        let _ = fs::remove_file(&no_run);
    }

    #[test]
    fn growing_past_the_memory_limit_is_refused() {
        // memory.grow(2000) is ~128 MiB, above the 64 MiB limit, so it returns -1.
        let path = module_file(
            "memory",
            r#"(module
                 (memory 1)
                 (func (export "run") (result i32)
                   (memory.grow (i32.const 2000))))"#,
        );
        let outcome = run_file(&path).expect("run");
        assert_eq!(outcome.exit_code, -1);
        let _ = fs::remove_file(&path);
    }
}

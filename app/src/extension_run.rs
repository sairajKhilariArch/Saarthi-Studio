//! Sandboxed execution for extension modules (ADR 0001).
//!
//! This is a wasmi interpreter, not a JIT: there is no ambient filesystem,
//! network, environment, clock or randomness. A module must export
//! `run() -> i32`, is bounded by a fuel budget and a memory/table limiter, and
//! can reach the workspace only through the host functions this module links.
//!
//! Host surface (the whole ABI, version 1):
//!
//! ```text
//! env.read(path_ptr, path_len, out_ptr, out_cap) -> i32   bytes written, or < 0
//! env.write(path_ptr, path_len, data_ptr, data_len) -> i32  0, or < 0
//! env.last_error(ptr, cap) -> i32                         message bytes written
//! ```
//!
//! Error codes: `-1` refused, `-2` invalid path, `-3` missing, `-4` too large,
//! `-5` not UTF-8 text, `-6` I/O failure. A host function is linked only when
//! the manifest holds the matching permission, so a module that imports one it
//! does not declare fails to instantiate instead of being silently allowed.
//!
//! Every call is checked against the same saved permission rules the agent
//! obeys, scoped to the open folder and the project, and recorded. An extension
//! has no UI to answer a prompt, so a rule that would `ask` is a refusal here:
//! unattended code never gets an approval it could not obtain.

use crate::permissions::{self, Action, Decision, Rule, Scope};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use wasmi::errors::{MemoryError, TableError};
use wasmi::{Config, Engine, Linker, Memory, Module, ResourceLimiter, Store};

/// Enough for useful pure computation, small enough that a runaway loop stops in
/// well under a second on this interpreter.
const FUEL: u64 = 50_000_000;
const MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const MAX_TABLE_ELEMENTS: u32 = 100_000;
const MAX_MODULE_BYTES: u64 = 8 * 1024 * 1024;
/// I/O is the expensive part of a run, so it is bounded separately from fuel:
/// 64 calls and 1 MiB per call. A module cannot use the sandbox as a bulk copy
/// tool or hold the blocking thread for long.
const MAX_HOST_CALLS: usize = 64;
const MAX_HOST_BYTES: usize = 1024 * 1024;
const MAX_PATH_BYTES: usize = 1024;

#[derive(Debug)]
pub(crate) struct Outcome {
    pub exit_code: i32,
    pub fuel_used: u64,
    pub elapsed_ms: u128,
    /// One entry per host call, in order. The caller writes these to the audit
    /// log after the run; the runner never touches the database itself.
    pub audit: Vec<HostCall>,
}

#[derive(Debug, Clone)]
pub(crate) struct HostCall {
    pub capability: &'static str,
    pub path: String,
    pub decision: Decision,
    pub scope: Scope,
    pub detail: String,
}

/// What an extension was granted for this run: the open folder, the project the
/// rules are resolved against, the rules themselves, and which of the two
/// workspace capabilities its manifest declared.
#[derive(Debug, Clone)]
pub(crate) struct HostGrant {
    pub root: PathBuf,
    pub project: String,
    pub rules: Vec<Rule>,
    pub read: bool,
    pub write: bool,
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

struct HostState {
    limits: Limits,
    grant: Option<HostGrant>,
    audit: Vec<HostCall>,
    calls: usize,
    last_error: String,
}

impl HostState {
    fn cap_reached(&mut self) -> bool {
        self.calls += 1;
        if self.calls > MAX_HOST_CALLS {
            self.last_error = format!(
                "The {MAX_HOST_CALLS}-call budget for workspace access is spent."
            );
            return true;
        }
        false
    }

    /// The rules are resolved exactly as the agent resolves them; `ask` is a
    /// refusal because nothing can answer for the extension.
    fn decide(&mut self, action: Action, target: &str, capability: &'static str) -> Result<(), i32> {
        let Some(grant) = self.grant.as_ref() else {
            return Err(-1);
        };
        let effective = permissions::resolve(grant.rules.iter(), action, target, &grant.project);
        let (decision, scope) = match effective {
            Some(effective) => (effective.decision, effective.scope),
            None => (action.default_decision(), Scope::Always),
        };
        let detail = match decision {
            Decision::Allow => "allowed".to_string(),
            Decision::Deny => "refused by a saved permission rule".to_string(),
            Decision::Ask => {
                "refused: an extension cannot ask for approval; add an allow rule in Settings → Permissions".to_string()
            }
        };
        self.audit.push(HostCall {
            capability,
            path: target.to_string(),
            decision,
            scope,
            detail: detail.clone(),
        });
        if decision == Decision::Allow {
            return Ok(());
        }
        self.last_error = format!("{capability} on \"{target}\" was {detail}.");
        Err(-1)
    }

    fn fail(&mut self, code: i32, message: impl Into<String>) -> i32 {
        self.last_error = message.into();
        code
    }
}

pub(crate) fn run_file(path: &Path, grant: Option<HostGrant>) -> Result<Outcome, String> {
    let meta = fs::metadata(path).map_err(|error| format!("The module cannot be read: {error}"))?;
    if meta.len() > MAX_MODULE_BYTES {
        return Err(format!(
            "The module is larger than the {} MiB limit.",
            MAX_MODULE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = fs::read(path).map_err(|error| format!("The module cannot be read: {error}"))?;
    run_bytes(&bytes, grant)
}

fn run_bytes(bytes: &[u8], grant: Option<HostGrant>) -> Result<Outcome, String> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &mut &bytes[..])
        .map_err(|error| format!("Not valid WebAssembly: {error}"))?;

    // Name the first import the grant cannot satisfy, before instantiation: a
    // linker error alone would not say which capability is missing.
    for import in module.imports() {
        let name = import.name();
        let allowed = match (import.module(), name) {
            ("env", "read") => grant.as_ref().map(|grant| grant.read).unwrap_or(false),
            ("env", "write") => grant.as_ref().map(|grant| grant.write).unwrap_or(false),
            ("env", "last_error") => true,
            _ => false,
        };
        if !allowed {
            return Err(format!(
                "This module imports \"{}\".\"{name}\", which the sandbox does not provide for this extension.",
                import.module()
            ));
        }
    }

    let mut store = Store::new(
        &engine,
        HostState {
            limits: Limits::default(),
            grant,
            audit: Vec::new(),
            calls: 0,
            last_error: String::new(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store
        .add_fuel(FUEL)
        .map_err(|error| format!("Fuel metering is unavailable: {error}"))?;

    let mut linker = Linker::new(&engine);
    linker
        .func_wrap("env", "read", |mut caller: wasmi::Caller<'_, HostState>, path_ptr: i32, path_len: i32, out_ptr: i32, out_cap: i32| -> i32 {
            host_read(&mut caller, path_ptr, path_len, out_ptr, out_cap)
        })
        .map_err(|error| format!("The sandbox could not be built: {error}"))?;
    linker
        .func_wrap("env", "write", |mut caller: wasmi::Caller<'_, HostState>, path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32| -> i32 {
            host_write(&mut caller, path_ptr, path_len, data_ptr, data_len)
        })
        .map_err(|error| format!("The sandbox could not be built: {error}"))?;
    linker
        .func_wrap("env", "last_error", |mut caller: wasmi::Caller<'_, HostState>, ptr: i32, cap: i32| -> i32 {
            host_last_error(&mut caller, ptr, cap)
        })
        .map_err(|error| format!("The sandbox could not be built: {error}"))?;

    let started = Instant::now();
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|error| format!("The module could not be instantiated: {error}"))?
        .start(&mut store)
        .map_err(|error| format!("The module could not start: {error}"))?;
    let run = instance
        .get_typed_func::<(), i32>(&store, "run")
        .map_err(|_| "The module must export run() -> i32.".to_string())?;
    let call = run.call(&mut store, ());
    let audit = std::mem::take(&mut store.data_mut().audit);
    let exit_code = call.map_err(|error| {
        // A trap leaves a little residual fuel, so the trap code is the reliable
        // signal — `fuel_consumed() == FUEL` does not hold exactly.
        let out_of_fuel = matches!(error.trap_code(), Some(wasmi::core::TrapCode::OutOfFuel));
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
        audit,
    })
}

// ---------------------------------------------------------------------------
// Host functions

fn guest_memory(caller: &wasmi::Caller<'_, HostState>) -> Option<Memory> {
    caller.get_export("memory").and_then(wasmi::Extern::into_memory)
}

/// Copy up to `max` bytes out of guest memory. A length the caller cannot have
/// produced is refused rather than allocated.
fn guest_bytes(
    caller: &wasmi::Caller<'_, HostState>,
    ptr: i32,
    len: i32,
    max: usize,
) -> Option<Vec<u8>> {
    if ptr < 0 || len < 0 || len as usize > max {
        return None;
    }
    let memory = guest_memory(caller)?;
    let mut buffer = vec![0u8; len as usize];
    memory.read(caller, ptr as usize, &mut buffer).ok()?;
    Some(buffer)
}

fn guest_path(caller: &wasmi::Caller<'_, HostState>, ptr: i32, len: i32) -> Option<String> {
    let bytes = guest_bytes(caller, ptr, len, MAX_PATH_BYTES)?;
    String::from_utf8(bytes).ok()
}

/// Read inside the open folder, with the agent's own path rules and the saved
/// permission rules applied before a single byte leaves the disk.
fn host_read(
    caller: &mut wasmi::Caller<'_, HostState>,
    path_ptr: i32,
    path_len: i32,
    out_ptr: i32,
    out_cap: i32,
) -> i32 {
    if caller.data_mut().cap_reached() {
        return -1;
    }
    let Some(relative) = guest_path(caller, path_ptr, path_len) else {
        return caller
            .data_mut()
            .fail(-2, "The path argument is not valid UTF-8.");
    };
    let (root, label) = match workspace_root(caller) {
        Some(root) => root,
        None => {
            return caller
                .data_mut()
                .fail(-2, "No folder is open, so there is no workspace to read.");
        }
    };
    if let Err(code) = caller
        .data_mut()
        .decide(Action::FsRead, &relative, "workspace.read")
    {
        return code;
    }
    let path = match crate::workspace::resolve_inside(&root, &relative) {
        Ok(path) => path,
        Err(error) => {
            let message = format!("{}: {}", label, error.message());
            return caller.data_mut().fail(-2, message);
        }
    };
    let meta = match fs::metadata(&path) {
        Ok(meta) if meta.is_file() => meta,
        _ => {
            let message = format!("{label}: no such file.");
            return caller.data_mut().fail(-3, message);
        }
    };
    let limit = (out_cap.max(0) as usize).min(MAX_HOST_BYTES);
    if meta.len() as usize > limit {
        let message = format!(
            "{label}: {} bytes does not fit the {limit}-byte read buffer.",
            meta.len()
        );
        return caller.data_mut().fail(-4, message);
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => return caller.data_mut().fail(-6, format!("{label}: {error}")),
    };
    if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
        let message = format!("{label}: the file is not UTF-8 text.");
        return caller.data_mut().fail(-5, message);
    }
    let Some(memory) = guest_memory(caller) else {
        return caller
            .data_mut()
            .fail(-1, "The module exports no memory for the result.");
    };
    if memory.write(&mut *caller, out_ptr.max(0) as usize, &bytes).is_err() {
        return caller
            .data_mut()
            .fail(-4, "The result does not fit the module's memory.");
    }
    bytes.len() as i32
}

/// Write inside the open folder. The default policy for a write is `ask`, which
/// an extension cannot answer, so this needs a saved allow rule — and a saved
/// deny still wins over it.
fn host_write(
    caller: &mut wasmi::Caller<'_, HostState>,
    path_ptr: i32,
    path_len: i32,
    data_ptr: i32,
    data_len: i32,
) -> i32 {
    if caller.data_mut().cap_reached() {
        return -1;
    }
    let Some(relative) = guest_path(caller, path_ptr, path_len) else {
        return caller
            .data_mut()
            .fail(-2, "The path argument is not valid UTF-8.");
    };
    let Some(data) = guest_bytes(caller, data_ptr, data_len, MAX_HOST_BYTES) else {
        return caller
            .data_mut()
            .fail(-4, format!("The content must be at most {MAX_HOST_BYTES} bytes."));
    };
    let Ok(content) = String::from_utf8(data) else {
        return caller
            .data_mut()
            .fail(-5, "The content is not UTF-8 text.");
    };
    let (root, label) = match workspace_root(caller) {
        Some(root) => root,
        None => {
            return caller
                .data_mut()
                .fail(-2, "No folder is open, so there is no workspace to write.");
        }
    };
    if let Err(code) = caller
        .data_mut()
        .decide(Action::FsWrite, &relative, "workspace.write")
    {
        return code;
    }
    match crate::workspace::write_text_file(&root, &relative, &content) {
        Ok(_) => 0,
        Err(error) => {
            let message = format!("{label}: {}", error.message());
            let code = match error.status() {
                axum::http::StatusCode::PAYLOAD_TOO_LARGE => -4,
                axum::http::StatusCode::NOT_FOUND => -3,
                axum::http::StatusCode::BAD_REQUEST | axum::http::StatusCode::FORBIDDEN => -2,
                _ => -6,
            };
            caller.data_mut().fail(code, message)
        }
    }
}

fn host_last_error(caller: &mut wasmi::Caller<'_, HostState>, ptr: i32, cap: i32) -> i32 {
    let message = caller.data().last_error.clone();
    if message.is_empty() {
        return 0;
    }
    let bytes = message.as_bytes();
    let limit = (cap.max(0) as usize).min(MAX_HOST_BYTES);
    let take = bytes.len().min(limit);
    let Some(memory) = guest_memory(caller) else {
        return -1;
    };
    if memory
        .write(&mut *caller, ptr.max(0) as usize, &bytes[..take])
        .is_err()
    {
        return -1;
    }
    take as i32
}

/// The root and a label for messages. `&Caller` already borrows the state, so
/// this takes the caller rather than `&HostState`.
fn workspace_root(caller: &wasmi::Caller<'_, HostState>) -> Option<(PathBuf, String)> {
    let grant = caller.data().grant.as_ref()?;
    // No folder is open: an empty root would resolve against the process cwd.
    if grant.root.as_os_str().is_empty() {
        return None;
    }
    let label = if grant.project.is_empty() {
        "workspace".to_string()
    } else {
        format!("workspace {}", grant.project)
    };
    Some((grant.root.clone(), label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{Action, Decision, Rule, Scope};
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

    fn workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "free-ai-studio-extension-ws-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("workspace");
        dir
    }

    fn grant(root: &Path, read: bool, write: bool) -> HostGrant {
        HostGrant {
            root: root.to_path_buf(),
            project: String::new(),
            rules: Vec::new(),
            read,
            write,
        }
    }

    fn rule(action: Action, target: &str, decision: Decision) -> Rule {
        Rule {
            action,
            target: target.to_string(),
            decision,
            scope: Scope::Always,
            project: String::new(),
            created_at: 0.0,
        }
    }

    #[test]
    fn a_minimal_module_returns_its_exit_code() {
        let path = module_file(
            "ok",
            r#"(module (func (export "run") (result i32) i32.const 7))"#,
        );
        let outcome = run_file(&path, None).expect("run");
        assert_eq!(outcome.exit_code, 7);
        assert!(outcome.fuel_used > 0);
        assert!(outcome.audit.is_empty());
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
        let error = run_file(&path, None).expect_err("fuel stops the loop");
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
        let error = run_file(&imported, None).expect_err("imports refused");
        assert!(error.contains("does not provide"), "{error}");
        let _ = fs::remove_file(&imported);

        let no_run = module_file(
            "no-run",
            r#"(module (func (export "other") (result i32) i32.const 0))"#,
        );
        let error = run_file(&no_run, None).expect_err("run export required");
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
        let outcome = run_file(&path, None).expect("run");
        assert_eq!(outcome.exit_code, -1);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_module_can_read_a_workspace_file_when_it_holds_the_permission() {
        let root = workspace("read");
        fs::write(root.join("hello.txt"), "hi there").expect("file");
        let path = module_file(
            "read",
            r#"(module
                 (import "env" "read" (func $read (param i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "hello.txt")
                 (func (export "run") (result i32)
                   (call $read (i32.const 0) (i32.const 9) (i32.const 64) (i32.const 100))))"#,
        );
        let outcome = run_file(&path, Some(grant(&root, true, false))).expect("run");
        assert_eq!(outcome.exit_code, 8, "eight bytes read");
        assert_eq!(outcome.audit.len(), 1);
        assert_eq!(outcome.audit[0].capability, "workspace.read");
        assert_eq!(outcome.audit[0].path, "hello.txt");
        assert_eq!(outcome.audit[0].decision, Decision::Allow);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_import_the_manifest_did_not_grant_is_refused() {
        let root = workspace("no-grant");
        let path = module_file(
            "read-no-grant",
            r#"(module
                 (import "env" "read" (func $read (param i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (func (export "run") (result i32)
                   (call $read (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))))"#,
        );
        let error = run_file(&path, Some(grant(&root, false, false))).expect_err("refused");
        assert!(error.contains("\"read\""), "{error}");
        assert!(error.contains("does not provide"), "{error}");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_declared_read_cannot_escape_the_folder() {
        let root = workspace("escape");
        fs::write(root.join("inside.txt"), "ok").expect("file");
        let path = module_file(
            "escape",
            r#"(module
                 (import "env" "read" (func $read (param i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "../secret.txt")
                 (func (export "run") (result i32)
                   (call $read (i32.const 0) (i32.const 13) (i32.const 64) (i32.const 100))))"#,
        );
        let outcome = run_file(&path, Some(grant(&root, true, false))).expect("run");
        assert_eq!(outcome.exit_code, -2, "path escape refused");
        assert_eq!(outcome.audit[0].decision, Decision::Allow, "the grant allowed the call, the path rule refused it");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_saved_deny_rule_stops_a_read_the_manifest_allows() {
        let root = workspace("deny");
        fs::write(root.join("secret.txt"), "token").expect("file");
        let path = module_file(
            "deny",
            r#"(module
                 (import "env" "read" (func $read (param i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "secret.txt")
                 (func (export "run") (result i32)
                   (call $read (i32.const 0) (i32.const 10) (i32.const 64) (i32.const 100))))"#,
        );
        let mut host = grant(&root, true, false);
        host.rules = vec![rule(Action::FsRead, "secret.txt", Decision::Deny)];
        let outcome = run_file(&path, Some(host)).expect("run");
        assert_eq!(outcome.exit_code, -1);
        assert_eq!(outcome.audit[0].decision, Decision::Deny);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_write_needs_an_allow_rule_and_is_audited() {
        let root = workspace("write");
        let source = r#"(module
             (import "env" "write" (func $write (param i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "out.txt")
             (data (i32.const 16) "written")
             (func (export "run") (result i32)
               (call $write (i32.const 0) (i32.const 7) (i32.const 16) (i32.const 7))))"#;
        let path = module_file("write", source);
        let outcome = run_file(&path, Some(grant(&root, false, true))).expect("run");
        assert_eq!(outcome.exit_code, -1, "the default policy for a write is ask");
        assert_eq!(outcome.audit[0].decision, Decision::Ask);
        assert!(!root.join("out.txt").exists());

        let mut host = grant(&root, false, true);
        host.rules = vec![rule(Action::FsWrite, "", Decision::Allow)];
        let outcome = run_file(&path, Some(host)).expect("run");
        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.audit[0].capability, "workspace.write");
        assert_eq!(
            fs::read_to_string(root.join("out.txt")).expect("written file"),
            "written"
        );
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_last_error_host_function_reports_why() {
        let root = workspace("last-error");
        let path = module_file(
            "last-error",
            r#"(module
                 (import "env" "read" (func $read (param i32 i32 i32 i32) (result i32)))
                 (import "env" "last_error" (func $error (param i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "missing.txt")
                 (func (export "run") (result i32)
                   (drop (call $read (i32.const 0) (i32.const 11) (i32.const 64) (i32.const 100)))
                   (call $error (i32.const 256) (i32.const 64))))"#,
        );
        let outcome = run_file(&path, Some(grant(&root, true, false))).expect("run");
        assert!(outcome.exit_code > 0, "an error message was written");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }
}

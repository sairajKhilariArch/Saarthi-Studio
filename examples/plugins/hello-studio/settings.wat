;; settings tool — reads the plugin's saved settings through `env.config` and
;; reports them back through `env.result`. A plugin that needs an endpoint, a
;; region or a project name reads it here instead of hard-coding it.
;;
;; Build with `wat2wasm settings.wat -o settings.wasm`, or from the repository:
;;   REGENERATE=1 cargo test --manifest-path app/Cargo.toml regenerate_example_tools -- --ignored
(module
  (import "env" "config" (func $config (param i32 i32) (result i32)))
  (import "env" "result" (func $result (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (local $len i32)
    (local.set $len (call $config (i32.const 0) (i32.const 4096)))
    (drop (call $result (i32.const 0) (local.get $len)))
    (local.get $len)))

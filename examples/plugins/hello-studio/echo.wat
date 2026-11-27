;; echo tool — reads its JSON arguments through `env.arguments` and reports them
;; back through `env.result`. This is the smallest honest tool: what the caller
;; passed is what comes back, so a wrong answer from it would be visible.
;;
;; Build with `wat2wasm echo.wat -o echo.wasm`, or from the repository:
;;   REGENERATE=1 cargo test --manifest-path app/Cargo.toml regenerate_example_tools -- --ignored
(module
  (import "env" "arguments" (func $arguments (param i32 i32) (result i32)))
  (import "env" "result" (func $result (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (local $len i32)
    (local.set $len (call $arguments (i32.const 0) (i32.const 4096)))
    (drop (call $result (i32.const 0) (local.get $len)))
    (local.get $len)))

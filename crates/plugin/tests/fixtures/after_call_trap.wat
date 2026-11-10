;; A plugin that runs the call and then traps: the session was in the store for the call, and must come back with
;; its path, its commands and its history when the call ends any way it can.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 1)
  (func (export "bimcraft_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "bimcraft_run") (param i32 i32 i32 i32) (result i64)
    (drop (call $execute (local.get 2) (local.get 3)))
    (unreachable)))
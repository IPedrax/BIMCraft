;; A plugin that declares a table far past a call's table limit: the call must be refused before it runs.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 1)
  (table (export "table") 100000 funcref)
  (func (export "bimcraft_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "bimcraft_run") (param i32 i32 i32 i32) (result i64) (i64.const 0)))
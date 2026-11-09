;; A plugin that grows its function table far past a call's table limit: the call must be an error, and the
;; host's memory must not grow with it.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 1)
  (table (export "table") 1 funcref)
  (func (export "bimcraft_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "bimcraft_run") (param i32 i32 i32 i32) (result i64)
    (drop (table.grow 0 (ref.null func) (i32.const 1000000)))
    (i64.const 0)))
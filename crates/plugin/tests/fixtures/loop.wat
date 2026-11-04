;; A plugin that never returns: the fuel limit must stop it.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 1)
  (func (export "bimcraft_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "bimcraft_run") (param i32 i32 i32 i32) (result i64)
    (loop $again (br $again))
    (i64.const 0)))
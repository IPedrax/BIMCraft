;; A plugin that asks for more memory than one call may have.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 1100)
  (func (export "bimcraft_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "bimcraft_run") (param i32 i32 i32 i32) (result i64) (i64.const 0)))
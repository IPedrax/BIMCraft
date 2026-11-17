(module
  ;; Answers exactly what the host's update_check says: the reply is already the ABI's envelope.
  (import "bimcraft" "update_check" (func $check (result i64)))
  (memory (export "memory") 1)
  (func (export "bimcraft_alloc") (param i32) (result i32)
    i32.const 1024)
  (func (export "bimcraft_free") (param i32 i32))
  (func (export "bimcraft_run") (param i32 i32 i32 i32) (result i64)
    call $check))

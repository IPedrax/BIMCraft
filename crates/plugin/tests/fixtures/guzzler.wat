;; A plugin that runs a command and then hangs: the fuel limit stops it, and the error has to name the command
;; it was running, which the sandbox alone could not say.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 1)
  (func (export "bimcraft_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "bimcraft_run") (param $name_ptr i32) (param $name_len i32) (param $params_ptr i32) (param $params_len i32) (result i64)
    (drop (call $execute (local.get $params_ptr) (local.get $params_len)))
    (loop $again (br $again))
    (i64.const 0)))
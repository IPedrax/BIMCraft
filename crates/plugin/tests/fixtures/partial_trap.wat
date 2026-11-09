;; A plugin that changes the model and then traps: the whole call must be rolled back.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 1024))
  (func (export "bimcraft_alloc") (param $len i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $next))
    (global.set $next (i32.add (global.get $next) (i32.add (local.get $len) (i32.const 8))))
    (local.get $p))
  (func (export "bimcraft_run") (param $name_ptr i32) (param $name_len i32) (param $params_ptr i32) (param $params_len i32) (result i64)
    (drop (call $execute (local.get $params_ptr) (local.get $params_len)))
    (unreachable)))
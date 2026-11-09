;; A plugin that tries more commands than one call may run; the last reply is returned.
(module
  (import "bimcraft" "execute" (func $execute (param i32 i32) (result i64)))
  (memory (export "memory") 256)
  (global $next (mut i32) (i32.const 1024))
  (func (export "bimcraft_alloc") (param $len i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $next))
    (global.set $next (i32.add (global.get $next) (i32.add (local.get $len) (i32.const 8))))
    (local.get $p))
  (func (export "bimcraft_run") (param $name_ptr i32) (param $name_len i32) (param $params_ptr i32) (param $params_len i32) (result i64)
    (local $i i32)
    (local $last i64)
    (block $done
      (loop $again
        (br_if $done (i32.ge_u (local.get $i) (i32.const 1005)))
        (local.set $last (call $execute (local.get $params_ptr) (local.get $params_len)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $again)))
    (local.get $last)))
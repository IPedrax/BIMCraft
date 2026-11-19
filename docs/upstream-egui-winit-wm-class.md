# Апстрим-патч: пустой instance в `WM_CLASS` (`egui-winit`)

Заготовка для PR в `emilk/egui`. Вендоринг в BIMCraft: `vendor/egui-winit` + `[patch.crates-io]` в корневом
`Cargo.toml` (см. [packaging.md](areas/packaging.md), пункт про `egui-winit`).

## Что не так

`egui-winit` 0.36.2, `crates/egui-winit/src/lib.rs`: `app_id` уходит как `general`, а `instance` — пустой.

```rust
#[cfg(all(feature = "wayland", target_os = "linux"))]
if let Some(app_id) = _app_id {
    use winit::platform::wayland::WindowAttributesExtWayland as _;
    window_attributes = window_attributes.with_name(app_id, "");
}
```

`winit` 0.30.13 хранит имя в общем для Wayland и X11 поле `platform_specific.name`, а X11-бэкенд строит из него
`WM_CLASS` как `(instance, general)`. Итог на X11: `WM_CLASS(STRING) = "", "io.github.ipedrax.bimcraft"` —
пустой `res_name` нарушает ICCCM и ломает сопоставление окон с приложениями. Например, GNOME Shell 46 сначала
сверяет instance с таблицей `StartupWMClass`, и пустое значение попадает в ключ `""`, который создаёт любой ярлык
с пустым `StartupWMClass=` (у Flatpak-ярлыка Remmina он пустой): окно BIMCraft привязывается к чужой иконке.

Wayland-бэкенд берёт только `name.general`, поэтому второй аргумент на Wayland ни на что не влияет.

## Патч

```diff
-        window_attributes = window_attributes.with_name(app_id, "");
+        window_attributes = window_attributes.with_name(app_id.as_str(), app_id.as_str());
```

`with_name(general, instance)` принимает `impl Into<String>`; `app_id` — `String`, `.as_str()` нужен, чтобы
не переместить его.

## Текст PR (английский)

Title: `egui-winit: set a non-empty WM_CLASS instance on X11`

Body:

```text
On Linux X11 a window gets `WM_CLASS(STRING) = "", "<app_id>"`: `ViewportBuilder::app_id` is passed to winit as
`general`, while `instance` is an empty string. winit keeps the name in `platform_specific.name`, which the X11
backend uses to build `WM_CLASS` as `(instance, general)`.

An empty `res_name` violates ICCCM and breaks matching windows to applications. For example, GNOME Shell 46 checks
the instance against the `StartupWMClass` table before the class, and an empty value lands on the `""` key that any
launcher with an empty `StartupWMClass=` creates (Remmina's Flatpak launcher has one): the window is bound to the
wrong icon and the right one shows no window.

Fix: pass `app_id` as the instance too. Wayland reads only `general`, so its behaviour does not change.

To check: build an eframe example on X11, `xprop WM_CLASS` — both fields equal `app_id`; in GNOME the window lands
on its own launcher.
```

## Проверка у себя

```
xprop -id <окно> WM_CLASS
# WM_CLASS(STRING) = "io.github.ipedrax.bimcraft", "io.github.ipedrax.bimcraft"
```

## Удаление вендоринга

Когда релиз `egui-winit` содержит этот патч: обновить `egui`/`eframe`, удалить `vendor/egui-winit` и
`[patch.crates-io]` из корневого `Cargo.toml`, пересобрать.

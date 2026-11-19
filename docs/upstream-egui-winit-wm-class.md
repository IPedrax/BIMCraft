# Upstream patch: an empty `WM_CLASS` instance (`egui-winit`)

A draft for a PR in `emilk/egui`. BIMCraft vendors the crate meanwhile: `vendor/egui-winit` plus the
`[patch.crates-io]` entry in the root `Cargo.toml` (see [packaging.md](areas/packaging.md), the `egui-winit` bullet).

## What is wrong

`egui-winit` 0.36.2, `crates/egui-winit/src/lib.rs`: `app_id` goes in as `general`, while `instance` is empty.

```rust
#[cfg(all(feature = "wayland", target_os = "linux"))]
if let Some(app_id) = _app_id {
    use winit::platform::wayland::WindowAttributesExtWayland as _;
    window_attributes = window_attributes.with_name(app_id, "");
}
```

`winit` 0.30.13 keeps the name in `platform_specific.name`, a field shared by Wayland and X11, and the X11
backend builds `WM_CLASS` from it as `(instance, general)`. The result on X11 is
`WM_CLASS(STRING) = "", "io.github.ipedrax.bimcraft"`: an empty `res_name` violates ICCCM and breaks matching
windows to applications. For example, GNOME Shell 46 checks the instance against the `StartupWMClass` table
before the class, and an empty value lands on the `""` key that any launcher with an empty `StartupWMClass=`
creates (Remmina's Flatpak launcher has one): the BIMCraft window is bound to the wrong icon.

The Wayland backend reads only `name.general`, so the second argument changes nothing there.

## The patch

```diff
-        window_attributes = window_attributes.with_name(app_id, "");
+        window_attributes = window_attributes.with_name(app_id.as_str(), app_id.as_str());
```

`with_name(general, instance)` takes `impl Into<String>`; `app_id` is a `String`, and `.as_str()` keeps it from
being moved.

## PR text (English)

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

## Checking locally

```
xprop -id <window> WM_CLASS
# WM_CLASS(STRING) = "io.github.ipedrax.bimcraft", "io.github.ipedrax.bimcraft"
```

## Removing the vendored copy

Once a release of `egui-winit` carries this patch: bump `egui`/`eframe`, delete `vendor/egui-winit` and the
`[patch.crates-io]` section of the root `Cargo.toml`, and rebuild.

# Пример плагина: Area Summary

Плагин BIMCraft на гостевом SDK. Читает помещения командой `elements`, складывает `area_m2` и умеет
записать итог текстовой заметкой одной командой — это один шаг undo. В проекте без помещений возвращает
ошибку; вызов при этом ничего не меняет.

## Сборка

```sh
rustup target add wasm32-unknown-unknown
cargo build --manifest-path plugins/area-summary/Cargo.toml --target wasm32-unknown-unknown --release
cp plugins/area-summary/target/wasm32-unknown-unknown/release/area_summary.wasm plugins/area-summary/
```

Готовый `area_summary.wasm` уже лежит рядом с манифестом: он собран из этого исходника, и интеграционный
тест `crates/plugin/tests/sample.rs` запускает именно его.

## Как попробовать

```sh
BIMCRAFT_PLUGINS="$PWD/plugins" cargo run -q -p bimcraft-cli -- run --sample --cmd 'plugin:area-summary:summary'
BIMCRAFT_PLUGINS="$PWD/plugins" cargo run -q -p bimcraft-cli -- run --sample \
  --cmd 'plugin:area-summary:write_note {"at":[0,0]}' --cmd undo
```

В десктопе папку `plugins` можно указать переменной `BIMCRAFT_PLUGINS`; плагин появится на вкладке
«Areas» в ленте, а его состояние и журнал — в Manage > Plugins.

## Что показывает пример

- `plugin.json`: id, версия, ABI 1, разрешения (здесь пустые), функции с JSON-схемой параметров и лентой.
- `src/lib.rs`: `execute` (команды вместо прямого доступа к модели), `info_log` (журнал) и `export!`
  (функции модуля и экспорты ABI).
- Ошибка: `Err` из обработчика откатывает весь вызов.

Подробности контракта — [docs/areas/plugins.md](../../docs/areas/plugins.md).
# rgine

Модульный 2D/3D игровой движок на Rust с ECS-архитектурой.

## Цели MVP

- headless runtime;
- собственный archetype ECS;
- `wgpu` + Render Graph;
- `winit` и Windows/Linux/macOS/Android;
- Rapier через `PhysicsBackend`;
- GUID-based asset pipeline;
- RON scenes/prefabs + migrations;
- egui editor;
- launcher;
- CLI-first workflow;
- тестовый 3D platformer.

## Первый запуск

```bash
cargo check --workspace
cargo test --workspace
cargo run -p engine-cli -- --help

## Лицензия

Двойная лицензия, на выбор пользователя:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE));
- MIT ([LICENSE-MIT](LICENSE-MIT)).

`SPDX: MIT OR Apache-2.0`. Вклад в проект принимается на тех же условиях, без отдельного соглашения.
```

Пустое окно - критерий выхода фазы 0:

```bash
cargo run -p engine-app --example empty_window
```

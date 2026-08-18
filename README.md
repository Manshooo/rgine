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

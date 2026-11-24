# rgine

Модульный 3D-движок на Rust с ECS-архитектурой. Поддержка 2D ограничена игровым UI: HUD, меню и экранными элементами управления.

## Цели MVP

- headless runtime;
- собственный archetype ECS;
- `wgpu` + Render Graph;
- `winit` и Windows/Linux/macOS/Android;
- Rapier через `PhysicsBackend`;
- GUID-based asset pipeline;
- RON scenes/prefabs + migrations;
- egui editor;
- [декларативный игровой UI с веб-подобной вёрсткой](docs/ROADMAP.md#game-ui);
- launcher;
- CLI-first workflow;
- тестовый 3D platformer.

## Требования для сборки

- Установите Rust и Cargo с помощью rustup. Проект использует канал `stable`. [Установка rustup](https://rustup.rs/)
- Для сборки под Windows с тулчейном MSVC установите Visual Studio Build Tools. В Visual Studio Installer выберите **Desktop development with C++** и проверьте, что выбраны компоненты **MSVC x64/x86 build tools** и **Windows SDK**. [Инструкция по настройке MSVC для Rust](https://rust-lang.github.io/rustup/devel/installation/windows-msvc.html)

## Первый запуск

```bash
cargo check --workspace
cargo test --workspace
```

Пустое окно можно открыть командой:

```bash
cargo run -p engine-app --example empty_window
```

После закрытия окна приложение завершится.

Пример `clear_window` выводит цветной кадр через `wgpu` и Render Graph.

```bash
cargo run --locked -p engine-app --example clear_window
```

Откроется окно с синей заливкой. Измените его размер, чтобы проверить перерисовку.

Воспроизведение ввода можно проверить без окна.

```bash
cargo test --locked -p engine-app --test input_replay
```

Тест записывает ввод по шагам симуляции, сериализует запись в JSON и воспроизводит её на заново собранной симуляции. Затем он сравнивает итоговое состояние ECS.

## Лицензия

Двойная лицензия, на выбор пользователя:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE));
- MIT ([LICENSE-MIT](LICENSE-MIT)).

`SPDX: MIT OR Apache-2.0`. Вклад в проект принимается на тех же условиях, без отдельного соглашения.

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
- редактор сцен с иерархией объектов и инспектором свойств;
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

Пример с процедурными 3D-мешами запускается отдельно.

```bash
cargo run --locked -p engine-app --example procedural_scene
```

Стрелки вращают камеру вокруг сцены, **W** и **S** меняют расстояние, а **Пробел** добавляет или убирает куб. При изменении размера окна камера обновляет пропорции кадра.

Воспроизведение ввода можно проверить без окна.

```bash
cargo test --locked -p engine-app --test input_replay
```

Тест записывает ввод по шагам симуляции, сериализует запись в JSON и воспроизводит её на заново собранной симуляции. Затем он сравнивает итоговое состояние ECS.

## Выбор графического API

На Windows движок сначала пробует DX12, на macOS — Metal, на Linux и Android — Vulkan. При ошибке инициализации он пробует следующий доступный API.

Для диагностики можно ограничить автоматический выбор переменной `WGPU_BACKEND`. В PowerShell можно явно выбрать DX12 для текущего запуска.

```powershell
$env:WGPU_BACKEND = 'dx12'
cargo run --locked -p engine-app --example clear_window
Remove-Item Env:WGPU_BACKEND
```

Переменная `RUST_LOG=engine_render=info` включает сообщения о неудачных попытках и выбранном GPU-бэкенде в примерах приложения.

На совместимом адаптере движок выбирает bindless и записывает режим в тот же лог. Чтобы сравнить изображение процедурной сцены в режиме classic, запустите пример с `RGINE_FORCE_CLASSIC=1`. На адаптере без нужных возможностей движок автоматически использует classic.

```powershell
$env:RGINE_FORCE_CLASSIC = '1'
cargo run --locked -p engine-app --example procedural_scene
Remove-Item Env:RGINE_FORCE_CLASSIC
```

## Лицензия

Двойная лицензия, на выбор пользователя:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE));
- MIT ([LICENSE-MIT](LICENSE-MIT)).

`SPDX: MIT OR Apache-2.0`. Вклад в проект принимается на тех же условиях, без отдельного соглашения.

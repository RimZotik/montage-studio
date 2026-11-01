# Montage Studio

Рабочее название. Настольное приложение для Windows для монтажа видео вместе с Claude Code:
пользователь собирает ролик на привычном монтажном столе (дорожки, клипы, слои), Claude — в чате
и терминале рядом, через инструменты студии; оба правят один и тот же файл проекта, ролик
собирается рендером по нему.

Статус: 0.6 — главная, компоненты, тихие обновления, проект с историей и отменой, чат с
Claude, медиа и правила, монтажный стол с плеером. Версии 0.Y; 1.0 — MVP.

## Запуск

- Приложение: `npm install`, затем `npm run tauri dev`.
- Только интерфейс в браузере (образцы данных вместо ядра): `npm run dev`, открыть http://127.0.0.1:1430.
- Проверки: `npm run typecheck`, `npm test`, в `src-tauri/` — `cargo test`, `cargo clippy --all-targets -- -D warnings`.

## Документация

| Тема | Файл |
| --- | --- |
| Устройство приложения, стек, папки | [docs/architecture.md](docs/architecture.md) |
| Компоненты: Claude Code, Node.js, движок рендера, ffmpeg | [docs/components.md](docs/components.md) |
| Обновления приложения и выпуск версии | [docs/updates.md](docs/updates.md) |
| Связь с Claude: чат, терминал, лимиты, инструменты MCP | [docs/claude-bridge.md](docs/claude-bridge.md) |
| Слой помощника: правила студии, память, карточки предложений | [docs/assistant-layer.md](docs/assistant-layer.md) |
| Формат проекта и таймлайна | [docs/project-format.md](docs/project-format.md) |
| Инструменты пользователя, библиотека, каталог | [docs/extensions.md](docs/extensions.md) |
| Интерфейс: окно, меню, страницы, горячие клавиши | [docs/ui.md](docs/ui.md) |
| Оформление: цвета, шрифты, элементы, анимации | [docs/design.md](docs/design.md) |
| Порядок работ | [docs/roadmap.md](docs/roadmap.md) |

Макеты — `design/mockups/` (открыть в браузере): [главная](design/mockups/home.html). Остальные страницы — по мере работы над ними.

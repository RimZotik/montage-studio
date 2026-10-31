# Montage Studio

Рабочее название. Настольное приложение для Windows для монтажа видео вместе с Claude Code:
пользователь собирает ролик на привычном монтажном столе (дорожки, клипы, слои), Claude — в чате
и терминале рядом, через инструменты студии; оба правят один и тот же файл проекта, ролик
собирается рендером по нему.

Статус: проектирование. Кода пока нет.

## Документация

| Тема | Файл |
| --- | --- |
| Устройство приложения, стек, папки | [docs/architecture.md](docs/architecture.md) |
| Компоненты: Claude Code, Node.js, движок рендера, ffmpeg | [docs/components.md](docs/components.md) |
| Связь с Claude: чат, терминал, правила, инструменты | [docs/claude-bridge.md](docs/claude-bridge.md) |
| Формат проекта и таймлайна | [docs/project-format.md](docs/project-format.md) |
| Интерфейс: окно, меню, страницы, горячие клавиши | [docs/ui.md](docs/ui.md) |
| Оформление: цвета, шрифты, элементы, анимации | [docs/design.md](docs/design.md) |
| Порядок работ | [docs/roadmap.md](docs/roadmap.md) |

Макет всех экранов — [design/mockups/studio.html](design/mockups/studio.html) (открыть в браузере).

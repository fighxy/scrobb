# Ядро плеера Scrobb (каркас)

> Статус: **каркас**. Решение «свой плеер сейчас не писать» остаётся в силе ([local-players-research.md](../../docs/local-players-research.md) §6.5). Каркас закрепляет архитектуру из [player-architecture.md](../../docs/player-architecture.md) и выделяет код, общий со Scrobb: протокол, правило скроббла, синхронизацию.

## Структура

```
player/core/                 Cargo workspace
  crates/
    scrobb-api/              DTO родного API Scrobb (§18.3, §18.4)            ─┐
    scrobb-rules/            правило скроббла и «сейчас играет» (§18.7)         │ ярус Scrobb:
    scrobb-client/           клиент API без ввода-вывода (sans-IO), повторы     │ собирается под wasm32
    scrobb-sync/             синхронизация рейтингов и плейлистов (заготовка)  ─┘
    player-domain/           треки, состояние, окна списков                    ─┐ общий ярус плеера:
    player-playback/         очередь, повтор                                    │ собирается под wasm32
    player-history/          история и превращение прослушиваний в скробблы   ─┘
    player-ports/            порты: звук, медиасессия, файлы, секреты, сеть, часы ─┐
    player-engine/           звуковой движок (заготовка)                          │ нативный
    player-library/          библиотека, SQLite, поиск (заготовка)                │ ярус плеера
    player-metadata/         MusicBrainz, AcoustID, кодировки (заготовка)         │
    player-plugins/          хост WASM-плагинов (заготовка)                       │
    player-core/             фасад: команды → состояние и события               ─┘
  bindings/
    ffi/                     → Swift и Kotlin для нативных оболочек (UniFFI, позже)
    wasm/                    → npm-пакет для веб-приложения Scrobb и расширения (wasm-bindgen, позже)
  scripts/check.sh           формат, clippy, тесты, сборка под wasm32
../../contracts/             языконезависимые контракты Scrobb (тест-векторы)
```

## Ярусы

| Ярус | Крейты | Кто использует | Ограничения |
|---|---|---|---|
| **Scrobb** | `scrobb-api`, `scrobb-rules`, `scrobb-client`, `scrobb-sync` | Плеер; десктоп-клиент Scrobb на Tauri (тоже Rust, [architecture.md](../../docs/architecture.md) §18.12); веб-приложение и расширение через `bindings/wasm` | Без ввода-вывода, системного времени и потоков; собирается под `wasm32-unknown-unknown`; не зависит от `player-*` |
| **Плеер, общий** | `player-domain`, `player-playback`, `player-history` | Ядро плеера; при желании — веб (например, веб-плеер на сайте) | Те же, что у яруса Scrobb |
| **Плеер, нативный** | `player-ports`, `player-engine`, `player-library`, `player-metadata`, `player-plugins`, `player-core` | Только плеер | Всё платформенное — только через порты |
| **Привязки** | `bindings/ffi`, `bindings/wasm` | Оболочки на SwiftUI и Compose; веб и расширение | Наружу — только фасад `player-core` (ffi) и ярус Scrobb (wasm) |

## Правила зависимостей

1. `scrobb-*` не зависят от `player-*`: ярус Scrobb можно забрать в десктоп-клиент или в веб без плеера.
2. Внизу — `scrobb-api`, контракт с бэкендом. Сейчас DTO написаны вручную по [architecture.md](../../docs/architecture.md) §18.3–§18.4; позже они генерируются из OpenAPI бэкенда.
3. Модули плеера не зависят друг от друга «по горизонтали». Общее — только `player-domain` и `player-ports`; связывает модули только `player-core`.
4. Время, сеть, файлы и секреты — только через порты (`player-ports`) или в стиле sans-IO: `scrobb-client` строит запросы, а выполняет их вызывающий код.
5. Список крейтов, обязанных собираться под wasm32, — в `scripts/check.sh` (`WEB_SAFE`); проверка падает, если в них появится несовместимая зависимость.

## Связь с веб-приложением Scrobb

- **Протокол — один на все клиенты.** DTO родного API лежат в `scrobb-api`, а `scrobb-client` строит и разбирает запросы, не выполняя их. Поэтому один код работает в плеере (порт `HttpTransport`), в Tauri-клиенте и в браузере (`fetch`).
- **Правило скроббла проверяется по одним векторам.** `scrobb-rules` прогоняется по [`contracts/scrobble-rules/v1.json`](../../contracts/scrobble-rules/v1.json). Эти же векторы должны проходить бэкенд (повторная проверка, §11.1, шаг 6) и TypeScript-код расширения.
- **Синхронизация.** `scrobb-sync` описывает рейтинги и «любимые», которые веб-приложение будет показывать и править. Для этого нужны эндпоинты синхронизации на сервере и общая с бэкендом нормализация ключа трека (`contracts/`, `match-key` — план).
- **Что нужно от бэкенда, когда плеер появится:**
  - новое значение `player` в `source.kind` и `client_kind` (сейчас только `extension` и `desktop`, §18.3);
  - эндпоинты синхронизации;
  - OpenAPI, из которого генерируются DTO.
- **Открытый вопрос:** [architecture.md](../../docs/architecture.md) §18.12 планирует `scrobbler-core` на TypeScript. Каркас показывает альтернативу — правило и протокол на Rust с поставкой в веб через wasm. Решать вместе с выбором стека десктоп-клиента (§16, п. 42).

## Сборка и проверки

```sh
cd player/core
scripts/check.sh
```

Нужен Rust 1.85+ (edition 2024). Для проверки под wasm: `rustup target add wasm32-unknown-unknown`; без этой цели шаг пропускается с предупреждением.

## Что дальше

По порядку из [player-architecture.md](../../docs/player-architecture.md) §9:
1. Спайк движка: Symphonia + cpal в `player-engine`, фоновое воспроизведение в минимальных оболочках на Android и iOS.
2. UniFFI в `bindings/ffi`: Swift и Kotlin, callback-интерфейсы для `Observer` и портов.
3. Библиотека: lofty, SQLite + FTS5, окна списков.
4. `bindings/wasm`: wasm-bindgen и npm-пакет для веба и расширения.

## Лицензия

Не выбрана: это открытый вопрос о лицензии своих клиентов ([architecture.md](../../docs/architecture.md) §16, п. 41). Пока везде `publish = false`.

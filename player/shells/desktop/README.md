# Scrobb Player для компьютера (каркас)

Десктоп-оболочка плеера для Windows, macOS и Linux.
- Окно — **Tauri 3 (alpha)** с системным WebView (`tauri-runtime-wry`), интерфейс на **React и TypeScript**.
- Ядро [`player/core`](../../core/README.md) подключено как обычный Rust-крейт, без FFI: окно говорит с ним командами Tauri и событиями.
- Почему так — [player-architecture.md](../../../docs/player-architecture.md) §5.1.

## Что уже умеет

- **Добавить файлы** (диалог или аргументы командной строки — «Открыть с помощью»). Теги читаются через `player-library::tags` (lofty): название, артисты, альбом, номер, длительность, ReplayGain.
- **Играть очередь:**
  - двойной щелчок по треку;
  - пауза, «назад», «вперёд», перемотка ползунком (позиция между снимками ядра движется сама);
  - громкость; повтор — выкл, очередь, трек.
- **История** — последние прослушивания из `history.db` (SQLite в каталоге данных приложения) с исходом и пометкой «засчитано».
- **Scrobb:**
  - привязка по коду (RFC 8628): код в окне, страница привязки открывается в браузере;
  - состояние отправки и «в очереди N»;
  - отключение.
  Токен — в хранилище секретов ОС (`keyring`). Сервер по умолчанию `https://scrobb.ru`, для разработки — переменная `SCROBB_BASE_URL`.
- **Окно подстраивается под ширину:** на узком библиотека и история становятся вкладками. Тёмная и светлая темы — по системе.

## Устройство

```
src/                     интерфейс (React)
  core.ts                типы ядра в JSON и интерфейс Backend
  tauri.ts               Backend в приложении: команды из src-tauri/src/bridge.rs
  mock.ts                Backend в браузере: подменное ядро для вёрстки и снимков
  usePlayer.ts           состояние окна из снимка и событий ядра
  components/            Library (виртуализированный список), Transport, History, ScrobbLink
src-tauri/               Rust-часть (свой Cargo workspace, MSRV 1.95)
  src/bridge.rs          команды dispatch / snapshot / add_files / recent_plays, события core://event
  src/adapters.rs        порты ядра: SystemClock, LocalFiles, UreqHttp, KeyringSecrets
  tauri.conf.json, capabilities/default.json
scripts/check.sh         проверки оболочки
```

- **Формат JSON** между окном и ядром — перечисления вида `{ "type": …, "data": … }`. Он закреплён тестом ядра `player-core/tests/json.rs`, TS-типы — в `src/core.ts`.
- **Отдельный workspace.** Ядро живёт в своём workspace со своим MSRV (1.89). Оболочке нужен Rust 1.95 (требование Tauri 3), а также GTK и WebKit на Linux. Поэтому у неё свой workspace и своя проверка.

## Запуск

Нужны Rust 1.95+, Node 22+ и pnpm. На Linux ещё `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`, `libasound2-dev`.

```sh
cd player/shells/desktop
pnpm install
pnpm tauri dev                   # приложение с горячей перезагрузкой интерфейса
pnpm dev                         # только интерфейс в браузере, на подменном ядре
#   ?demo=playing, ?demo=linking, ?demo=empty — готовые состояния
pnpm tauri build                 # установщики: MSI/NSIS, DMG, deb/rpm/AppImage
scripts/check.sh                 # типы, тесты, сборка интерфейса; fmt, clippy, тесты Rust
```

## Как проверено

- **Тесты:** `scripts/check.sh` (vitest — форматирование и плавная позиция; Rust — адаптеры и добавление файлов) и проверки ядра `player/core/scripts/check.sh`.
- **Интерфейс в Chromium (Playwright) на подменном ядре:** широкое и узкое окно, обе темы, окно привязки, пустая библиотека; сценарий «двойной щелчок → играет, пауза, повтор».
- **Собранное приложение под Xvfb** (WebKitGTK) с тремя файлами в аргументах на ALSA-устройстве `null`:
  - окно поднялось;
  - теги прочитаны;
  - очередь проиграна до конца;
  - в `history.db` три записи «дослушан», и окно их показывает.
  Реального звука и связи с Scrobb в контейнере нет: их покрывают тесты ядра с поддельными сервером и выводом.

## Чего пока нет

По порядку:
1. Медиаклавиши и системная медиасессия (SMTC, MPRIS, Now Playing) — адаптер порта `MediaSession` на `souvlaki`.
2. Сканер папок и база библиотеки (SQLite + FTS5) вместо списка в памяти; обложки.
3. Трей, автозапуск, единственный экземпляр, запоминание размера окна — плагины `single-instance`, `window-state`, `autostart`.
4. Автообновление с подписью (`updater`), подпись установщиков ([architecture.md](../../../docs/architecture.md) §16, п. 39).
5. Переход с альфы на Tauri 3.0 stable; общие дизайн-токены Scrobb вместо локальных.
6. Иконка — заглушка.

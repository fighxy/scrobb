# Scrobb

**Scrobb** — сервис скробблинга (учёта прослушанной музыки) для российского рынка, аналог [Last.fm](https://www.last.fm/), и **свой музыкальный плеер**. Сейчас в работе плеер: ядро на Rust и десктоп-приложение. Сервис пока проектируется.

**Ядро плеера**

[![Rust](https://img.shields.io/badge/Rust-edition%202024-CE422B?logo=rust&logoColor=white)](player/core/README.md)
[![Symphonia](https://img.shields.io/badge/Symphonia-0.6-6B7280)](https://github.com/pdeljanov/Symphonia)
[![cpal](https://img.shields.io/badge/cpal-0.18-6B7280)](https://github.com/RustAudio/cpal)
[![rubato](https://img.shields.io/badge/rubato-4-6B7280)](https://github.com/HEnquist/rubato)
[![lofty](https://img.shields.io/badge/lofty-0.25-6B7280)](https://github.com/Serial-ATA/lofty-rs)
[![SQLite](https://img.shields.io/badge/SQLite-rusqlite%200.41-003B57?logo=sqlite&logoColor=white)](https://github.com/rusqlite/rusqlite)
[![WebAssembly](https://img.shields.io/badge/WebAssembly-wasm32-654FF0?logo=webassembly&logoColor=white)](player/core/README.md#ярусы)

**Десктоп-приложение**

[![Tauri](https://img.shields.io/badge/Tauri-3.0%20alpha-24C8D8?logo=tauri&logoColor=white)](player/shells/desktop/README.md)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)](https://react.dev)
[![TypeScript](https://img.shields.io/badge/TypeScript-7-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org)
[![Vite](https://img.shields.io/badge/Vite-8-646CFF?logo=vite&logoColor=white)](https://vite.dev)
[![Tailwind CSS](https://img.shields.io/badge/Tailwind%20CSS-4-06B6D4?logo=tailwindcss&logoColor=white)](https://tailwindcss.com)
[![shadcn/ui](https://img.shields.io/badge/shadcn%2Fui-Radix-111111?logo=shadcnui&logoColor=white)](https://ui.shadcn.com)
[![Vitest](https://img.shields.io/badge/Vitest-5-6E9F18?logo=vitest&logoColor=white)](https://vitest.dev)
[![Playwright](https://img.shields.io/badge/Playwright-%D1%81%D1%86%D0%B5%D0%BD%D0%B0%D1%80%D0%B8%D0%B8-2EAD33)](https://playwright.dev)

**Сервис Scrobb (проектируется)**

[![Python](https://img.shields.io/badge/Python-3.14-3776AB?logo=python&logoColor=white)](docs/stack-rationale.md)
[![FastAPI](https://img.shields.io/badge/FastAPI-%D0%BC%D0%BE%D0%BD%D0%BE%D0%BB%D0%B8%D1%82-009688?logo=fastapi&logoColor=white)](docs/architecture.md)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-18-4169E1?logo=postgresql&logoColor=white)](docs/architecture.md)
[![procrastinate](https://img.shields.io/badge/%D0%BE%D1%87%D0%B5%D1%80%D0%B5%D0%B4%D1%8C-procrastinate-6B7280)](docs/architecture.md)
[![Valkey](https://img.shields.io/badge/Valkey-9-6983FF)](docs/architecture.md)
[![Next.js](https://img.shields.io/badge/Next.js-TypeScript-000000?logo=nextdotjs&logoColor=white)](docs/frontend-core.md)

> **Статус (25.10.2026).**
> - **Плеер — прототип.** Ядро и десктоп работают и покрыты тестами. Установщики для Windows, Linux и macOS собираются и проверяются на настоящих системах в GitHub Actions.
> - **Сервис** — документы и спецификации, кода бэкенда и сайта нет. Общий с ним код уже есть в ядре плеера: протокол, правило скроббла, ключ трека, синхронизация.

## Плеер

Свой плеер для локальной музыки со встроенным скробблингом в Scrobb. Зачем он и как устроен — [player-architecture.md](docs/player-architecture.md). Почему не только чужие плееры — [local-players-research.md](docs/local-players-research.md).

### Что уже умеет

- **Звук без компромиссов:**
  - бесшовные переходы (gapless) сэмпл в сэмпл;
  - ReplayGain по треку и альбому;
  - качественный ресемплинг и точная перемотка;
  - форматы MP3, AAC, FLAC, ALAC, Vorbis, WAV, AIFF.
- **Библиотека:**
  - папки-источники с фоновым сканированием, теги и обложки;
  - альбомы, артисты, поиск ⌘K без учёта регистра и «ё».
- **История и скробблинг:**
  - каждое прослушивание — в `history.db`, засчитанное — по правилу Scrobb;
  - офлайн-очередь с повторами;
  - привязка устройства кодом, без ручных токенов;
  - «сейчас играет».
- **Плейлисты:**
  - импорт и экспорт M3U8, XSPF, JSPF (ListenBrainz), CSV (TuneMyMusic, Soundiiz), TXT (Яндекс Музыка), импорт PLS;
  - перетаскивание треков;
  - трек, которого нет в библиотеке, остаётся в плейлисте и находится сам, когда файл появится.
- **Синхронизация плейлистов** между устройствами через аккаунт Scrobb:
  - правки по полям с гибридными часами, слияние без конфликтов;
  - клиент готов, эндпоинт сервера описан в спецификации.
- **Интерфейс** в духе Spotify и Deezer: тёмная и светлая темы, панель «Сейчас играет» с очередью, цвет обложки в шапках, узкое окно.
- **Установщики** — собираются в [GitHub Actions](.github/workflows/player-installers.yml) вручную или по тегу `v*`, тогда все файлы сразу в черновике релиза:
  - Windows — `setup.exe` без прав администратора, открывает плейлисты двойным щелчком, аудиофайлы не перехватывает;
  - Linux — deb и AppImage;
  - macOS — DMG для Apple Silicon.

Подробно — [README ядра](player/core/README.md) и [README десктопа](player/shells/desktop/README.md).

### Чего пока нет

- медиаклавиш и системной медиасессии;
- перемешивания;
- эквалайзера и Opus;
- подписи установщиков (Windows, Apple) и автообновления;
- мобильных оболочек (Android, iOS);
- сервера синхронизации — он появится вместе с бэкендом Scrobb.

Полный список — [README десктопа](player/shells/desktop/README.md#чего-пока-нет).

### Устройство

```mermaid
flowchart LR
  subgraph SH["Оболочки"]
    DESK["Десктоп<br/>Tauri 3 + React"]
    MOB["Android, iOS<br/>(позже, UniFFI)"]
    WEBX["Сайт и расширение Scrobb<br/>(позже, WebAssembly)"]
  end
  subgraph CORE["Ядро плеера (Rust)"]
    FAC["player-core<br/>команды → события"]
    ENG["engine<br/>Symphonia, gapless, ReplayGain"]
    LIB["library<br/>сканер, теги, обложки, поиск трека"]
    STO["history, playlists<br/>history.db, user.db (SQLite)"]
    ST["ярус Scrobb<br/>API, правило скроббла, клиент, синхронизация"]
  end
  SRV[("Сервер Scrobb<br/>скробблы, плейлисты")]
  DESK --> FAC
  MOB -.-> FAC
  WEBX -.-> ST
  FAC --> ENG
  FAC --> LIB
  FAC --> STO
  STO --> ST
  ST -- "HTTP через порт" --> SRV
```

- **Одно ядро на Rust** для всех платформ. Оболочки — нативные: десктоп на Tauri, мобильные — позже, через UniFFI.
- **Ярус Scrobb** (протокол, правило скроббла, ключ трека, синхронизация) не зависит от плеера и собирается под WebAssembly. Тот же код получат сайт и расширение.
- **Ввод-вывод — через порты:** звук, файлы, сеть, секреты, часы. Поэтому ядро тестируется без звуковой карты и сети.

### Запуск

Нужны Rust 1.95+ (ядро — 1.89+), Node 22+ и pnpm. На Linux ещё WebKitGTK и ALSA — список в [README десктопа](player/shells/desktop/README.md#запуск).

```sh
# ядро: формат, clippy, тесты, сборка под wasm32
cd player/core && scripts/check.sh

# десктоп
cd player/shells/desktop
pnpm install
pnpm tauri dev          # приложение
pnpm dev                # только интерфейс в браузере, на подменном ядре (?demo=playing, ?demo=sync…)
scripts/check.sh        # типы, тесты, сборка; fmt, clippy, тесты Rust

# сервер Scrobb в памяти для разработки: привязка и синхронизация плейлистов
cd player/core && cargo run -p player-testkit --example scrobb-dev-server -- --port 8787
SCROBB_BASE_URL=http://127.0.0.1:8787 pnpm tauri dev
```

### Как проверено

- **Ядро — 152 теста**, clippy без предупреждений, сборка общего яруса под wasm32. Среди тестов:
  - бесшовные переходы сэмпл в сэмпл и ресемплинг;
  - история и скробблинг на поддельном сервере;
  - форматы плейлистов на живых образцах из foobar2000, AIMP, VLC, ListenBrainz, Exportify;
  - синхронизация двух «устройств» с расходящимися часами.
- **Десктоп** — vitest и Rust-тесты оболочки, сценарии Playwright на подменном ядре, собранное приложение под Xvfb. Синхронизация плейлистов проверена двумя настоящими экземплярами приложения через dev-сервер.
- **Мутационная проверка:** намеренно внесённые поломки ловятся тестами.

## Сервис Scrobb

### Зачем

- **Last.fm с марта 2026 года недоступен из России**: по сообщениям пользователей, блокируются и сайт, и эндпоинты скробблинга (HTTP 403).
- **API ToS Last.fm разрешает только некоммерческое использование**, а данные из Last.fm ограничены лимитом в 100 MB и требуют атрибуции.

Поэтому ядро Scrobb — **собственный приём скробблов**, а Last.fm — дополнительный модуль, выключенный по умолчанию. Подробнее — [architecture.md](docs/architecture.md), §0 и §3.

### Этап 1 (MVP)

- **Свои скробблеры — основной способ подключения** (предложение):
  - **расширение браузера** для Chrome/Chromium-браузеров и Firefox с коннекторами Яндекс Музыки, VK Музыки, Звука и YouTube Music — в MVP;
  - **десктоп-приложение** для Windows и Linux, читающее системные медиасессии, — сразу после MVP, macOS позже.

  Вход — привязкой к аккаунту одной кнопкой, без ручных токенов; есть офлайн-очередь. Подробнее — [architecture.md](docs/architecture.md), §18. Свой плеер — отдельная линия, тот же протокол.
- **Собственный приём скробблов:**
  - родной API для своих клиентов;
  - для сторонних — API, совместимый с ListenBrainz, и эндпоинт `/2.0/`, совместимый с Last.fm (если не успеем в MVP — сразу после).

  Существующие клиенты (Web Scrobbler, Pano Scrobbler, rescrobbled, foobar2000 + foo_listenbrainz2, Navidrome) остаются вторым вариантом и отправляют прослушивания по API-токену.
- **Веб-приложение** — публичные профили, история, топы за периоды, графики, приватность.
- **Импорт из Last.fm** — разовый перенос истории и, по желанию, постоянная синхронизация. По умолчанию выключен.
- **Мотивация и социальное** — стрики, достижения, подписки на друзей, лидерборды среди друзей.
- **Telegram-бот** — привязка по одноразовой ссылке, статистика, уведомления.
- **Донаты** и **email** — подтверждение адреса и уведомления безопасности ([architecture.md](docs/architecture.md), §17).
- Уникальный публичный **@username** и неизменяемый внутренний id.

### Этап 2 и позже

- **Синхронизация плейлистов** плеера и плейлисты на сайте — клиент готов, спецификация эндпоинта — [playlists.md](docs/playlists.md), §8.
- **Рекомендации** с объяснением, почему рекомендовано.
- **Scrobb Pro**: 99 ₽ за 30 дней разовым платежом через ЮKassa, без автопродления. Ядро, свои данные и стрики остаются бесплатными.
- Позже — скробблинг **видео** и **игр из Steam**.

### Стек

| Часть | Выбор |
|---|---|
| Бэкенд | Python 3.14 + FastAPI, модульный монолит |
| Фоновые задачи | procrastinate — очередь и периодические задачи в PostgreSQL, за тонким интерфейсом `jobs` |
| База данных | PostgreSQL 18, таблица прослушиваний секционирована по месяцам |
| Метаданные музыки | Локальная копия core-данных MusicBrainz (CC0), API MusicBrainz — только для нового |
| Кэш, лимиты, лидерборды | Valkey 9 (один экземпляр) |
| Фронтенд | Next.js (TypeScript) |
| Свои клиенты | Плеер: ядро на Rust, десктоп на Tauri 3 + React (в работе). Расширение: TypeScript, WebExtension Manifest V3 (WXT); десктоп-скробблер: Tauri (предложение) |
| Файлы | S3-совместимое хранилище в РФ |
| Развёртывание | Docker Compose на хостинге в РФ (152-ФЗ), CI/CD — GitHub Actions |
| Наблюдаемость | Prometheus + Grafana, ошибки — self-hosted GlitchTip |

Почему именно так — [stack-rationale.md](docs/stack-rationale.md).

### Архитектура (упрощённо)

```mermaid
---
config:
  layout: dagre
---
flowchart LR
  subgraph U["Пользователи"]
    WEB["Веб-приложение (Next.js)"]
    EXT["Своё расширение браузера<br/>(основной клиент)"]
    DESK["Свой плеер и десктоп-скробблер<br/>(основные клиенты)"]
    THIRD["Сторонние скробблеры<br/>(ListenBrainz- и Last.fm-совместимые)"]
    TGU["Telegram"]
  end
  subgraph B["Бэкенд (FastAPI, модульный монолит)"]
    API["API"]
    ING["Ingest: приём скробблов<br/>родной API · совместимые API"]
    BILL["Биллинг: донаты, Pro"]
    WRK["Воркеры: статистика, стрики, достижения,<br/>импорт, уведомления, почта"]
    BOT["Telegram-бот"]
  end
  subgraph D["Данные (РФ)"]
    PG[("PostgreSQL: данные, очередь")]
    VK[("Valkey: кэш, лимиты")]
    S3[("S3")]
  end
  subgraph X["Внешние сервисы"]
    YK["ЮKassa"]
    LFM["Last.fm: импорт, обогащение<br/>(через egress-узел вне РФ, опц.)"]
    MAIL["Почтовый провайдер"]
  end

  WEB --> API
  EXT -- "родной API, токен устройства" --> ING
  DESK -- "родной API, токен устройства" --> ING
  THIRD -- "совместимые API, API-токен" --> ING
  TGU <-- "webhook, сообщения" --> BOT
  API --> BILL
  ING -- "задачи" --> WRK
  WRK -- "уведомления" --> BOT
  BILL <-- "платёж, webhook" --> YK
  WRK -- "импорт, обогащение" --> LFM
  WRK <-- "письма, webhook bounce" --> MAIL
  B == "чтение и запись" ==> D
```

## Код

| Путь | Что там |
|---|---|
| [player/core](player/core/README.md) | Ядро плеера на Rust: звуковой движок, очередь, библиотека, история и скробблинг, плейлисты и их синхронизация. Общий со Scrobb ярус (`scrobb-*`) собирается под WebAssembly |
| [player/shells/desktop](player/shells/desktop/README.md) | Десктоп-приложение: Tauri 3 (alpha) + React, Tailwind CSS 4, shadcn/ui |
| [contracts](contracts/README.md) | Языконезависимые тест-векторы для бэкенда, веба, расширения и Rust: правило скроббла, ключ трека, синхронизация плейлистов |

## Документация

**Плеер**
- [player-architecture.md](docs/player-architecture.md) — стек и архитектура плеера для Android, iOS, macOS, Windows и Linux: одно ядро на Rust, нативные оболочки.
- [player-history.md](docs/player-history.md) — история прослушиваний и скробблинг: формат, база, офлайн-очередь, привязка устройства, «сейчас играет».
- [playlists.md](docs/playlists.md) — плейлисты: стандарты (M3U, XSPF/JSPF, CSV), импорт и экспорт, хранение, синхронизация, спецификация эндпоинта для бэкенда.
- [local-players-research.md](docs/local-players-research.md) — локальные плееры для десктопа, Android и iOS: функции, сильные и слабые стороны, скробблинг с телефона.

**Сервис**
- [architecture.md](docs/architecture.md) — архитектура этапа 1:
  - модель данных и приём скробблов;
  - импорт Last.fm;
  - инфраструктура;
  - механики и экраны, дизайн-система;
  - монетизация;
  - Telegram-бот, email;
  - свои клиенты;
  - открытые вопросы.
- [backend-core.md](docs/backend-core.md) и [frontend-core.md](docs/frontend-core.md) — карты ядра бэкенда и фронтенда.
- [stack-rationale.md](docs/stack-rationale.md) — обоснование стека, сравнение с альтернативами и со стеком Last.fm.
- [music-services-ru.md](docs/music-services-ru.md) — музыкальные сервисы в РФ и способы получить историю прослушиваний.
- [ci-cd.md](docs/ci-cd.md) — CI/CD на бесплатных минутах GitHub Free, деплой и откат.
- [review.md](docs/review.md) — ревью проекта: риски, что пересмотреть, что уже сделано.

## Участие

Идеи, вопросы и предложения — через [Issues](../../issues).

Лицензия пока не выбрана ([architecture.md](docs/architecture.md), §16, п. 41).

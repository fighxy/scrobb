# Почему выбран именно такой стек

> Статус: **черновик** (03.10.2026). Обоснование к §6 «Технологический стек» в [architecture.md](architecture.md); сам стек зафиксирован там. Пометка **(не проверено)** означает, что факт не подтверждён первоисточником. Ссылки на источники собраны в конце, номера в тексте вида [N].

## Коротко

Первый этап Scrobb — это не стриминг под высокой нагрузкой. Ядро — **собственный приём скробблов**: основной путь — **свои клиенты** (расширение браузера и десктоп-приложение, architecture.md §18) через родной API, второй — сторонние плееры и скробблеры по ListenBrainz-совместимому и Last.fm-совместимому API (§3). Last.fm — дополнительный модуль: разовый импорт истории и опциональная синхронизация (§4). Позже добавятся Яндекс Музыка, VK и Звук. Сервис складывает прослушивания в одну большую таблицу-временной ряд и считает по ней статистику, стрики, достижения и рейтинги. Под такую задачу лучше всего подходит **Python 3.12 + FastAPI + arq**: асинхронный I/O для множества коротких запросов приёма и для импорта из внешних API, OpenAPI-контракт, который сразу подходит и вебу, и будущим мобильным приложениям, и прямой доступ к неофициальным Python-библиотекам российских сервисов. **PostgreSQL 16** с секционированием одним движком закрывает и историю, и аналитику, а **Valkey** (открытый BSD-форк Redis под эгидой Linux Foundation, два экземпляра — очередь и кэш) берёт на себя очередь, лимиты запросов и горячие лидерборды. **Next.js** даёт SSR публичных профилей и общую с будущим React Native-приложением базу на TypeScript и React. Всё это один человек разворачивает через **Docker Compose** на хостинге в РФ (152-ФЗ). У Last.fm стек другой (PHP/Java/C++, Hadoop), но он отражает масштаб 2000-х и десятки миллионов пользователей, а не маленькую команду 2026 года.

## Стек в одном списке

| Технология | Зачем, в одну-две строки |
|---|---|
| **Python 3.12** | Неофициальные клиенты Яндекс Музыки (`yandex-music`, с Ynison) и Звука (`zvuk-music`) есть только на Python, поэтому будущие провайдеры подключаются без отдельного сервиса. Плюс сильная экосистема для данных и рекомендаций |
| **FastAPI** + Pydantic v2 | async I/O для потока коротких запросов приёма скробблов и запросов к внешним API; Pydantic-схемы валидируют входящие прослушивания. OpenAPI генерируется из кода, а из него — типизированный TS-клиент для веба и мобильных приложений |
| **SQLAlchemy 2 (async) + Alembic** | Зрелая ORM и миграции. Для сложной аналитики (gaps-and-islands, `ON CONFLICT`) можно писать чистый SQL рядом |
| **arq** (очередь + cron; протокол Redis, работает на Valkey) | Тот же asyncio-код, что и в API. Есть уникальные job id, то есть одна синхронизация на аккаунт, и задачи по расписанию. Легче Celery. ⚠️ С октября 2025 проект в режиме «только поддержка» (см. «Риски») |
| **PostgreSQL 16** (секции по месяцам, `pg_partman`, `pg_trgm`, `citext`) | Один движок для OLTP и агрегатов. Идемпотентный импорт через `ON CONFLICT`, оконные функции для стриков, месячные секции под историю прослушиваний |
| **Valkey** (два экземпляра: очередь и кэш) | Вместо Redis из-за смены лицензии Redis в 2024 году (см. раздел «Valkey»). Брокер arq; token bucket под лимиты на токены клиентов, Last.fm (≈2–3 req/s, только импорт и опциональная синхронизация) и MusicBrainz (≤1 req/s); sorted sets для лидербордов; now playing с TTL |
| **Свои клиенты** (предложение, architecture.md §18.12): расширение — TypeScript + WebExtension MV3, сборка WXT [29]; десктоп — Tauri 2 [30] | Тот же TypeScript и сгенерированный из OpenAPI клиент, что у веба; общее ядро правил скроббла для обоих клиентов. Tauri — маленький установщик, нативный доступ к медиасессиям ОС через Rust, обязательная подпись обновлений |
| **Next.js (App Router, TypeScript)** + TanStack Query, Tailwind/shadcn, ECharts/Recharts | SSR публичных профилей (превью ссылок, SEO), интерактивные графики. TS/React общие с будущим Expo/React Native |
| **Docker Compose** на VPS или в облаке в РФ (Selectel / Yandex Cloud / VK Cloud / Timeweb) + Caddy | 152-ФЗ ст. 18 ч. 5: ПДн хранятся в РФ. Восемь сервисов (вместе с Telegram-ботом) поднимаются одной командой, Kubernetes не нужен |
| **GitHub Actions** → GHCR → деплой по SSH | Репозиторий уже на GitHub. Lint, тесты (pytest + testcontainers-postgres), сборка образов, миграции отдельным шагом |
| **Prometheus + Grafana** | Главный риск — приём или импорт, которые сломались без ошибок. Метрики латентности и `5xx` приёма, «замолчавших» токенов, глубины очередей, лага синхронизации Last.fm, ошибок `29`/`17` и алерты на них |
| **GlitchTip** (self-hosted, Sentry-совместимый) | Трекинг ошибок без отправки ПДн за рубеж (SaaS-Sentry — это трансграничная передача). Клиенты Sentry SDK работают без изменений |

## Подробное обоснование

### Чем нагружена система на этапе 1

| Задача | Характер нагрузки | Чем закрывается |
|---|---|---|
| **Приём скробблов — основной поток** (ListenBrainz-совместимый `submit-listens` и Last.fm 2.0-совместимый `/2.0/`, MVP) | Короткие POST-запросы, валидация, лимиты на токен, now playing, дедупликация | FastAPI + Pydantic-схемы; `INSERT … ON CONFLICT (user_id, played_at, alias_id)`; Valkey для токенов, лимитов и `playing_now`; большие `import` — в фоновую очередь |
| Импорт из Last.fm (backfill; опционально incremental и reconcile) | Много долгих HTTP-запросов, жёсткий глобальный лимит, ретраи, чекпоинты | async-воркеры arq, token bucket в Valkey, `sync_state` в PG, уникальный job id на аккаунт |
| Статистика и топы за периоды, отчёты | Агрегаты по временному ряду | Секционированная `listens`, rollup'ы `daily_activity` / `user_entity_monthly`, SQL `GROUP BY` / оконные функции |
| Стрики | gaps-and-islands по датам в TZ пользователя | Один SQL-запрос с `ROW_NUMBER()` по `daily_activity` (см. architecture.md §8.1) |
| Достижения | Событийная проверка порогов, ретроактивная выдача | transactional outbox в PG → обработчики в arq, идемпотентный upsert |
| Профили и приватность | Обычный CRUD + SSR публичных страниц | FastAPI + единый `can_view()`; Next.js SSR |
| Друзья и лидерборды | Выборка по десяткам–сотням id; горячие общие доски | SQL `WHERE user_id = ANY($friends)` по rollup'у; Valkey ZSET для общих досок и кэша |
| Совместимость и рекомендации (этап 2) | Векторы по топ-артистам, позже коллаборативная фильтрация | Python (numpy / scipy / implicit и т. п.) прямо в воркерах, без отдельного ML-стека |
| Будущие провайдеры РФ | Неофициальные API, токены, realtime (Ynison) | Python-библиотеки вызываются напрямую, как ещё один класс `ListeningSourceProvider` |
| Мобильные приложения | Нужен стабильный API-контракт | API-first: тот же REST/JSON + OpenAPI, сгенерированный клиент |

Вывод: узкое место первого этапа — **I/O к внешним API и SQL-агрегаты**. CPU-тяжёлых вычислений на запрос нет. Поэтому язык выбирали по экосистеме и скорости разработки, а не по сырой производительности.

### Python 3.12 + FastAPI

- **Решающий аргумент — провайдеры РФ.** Неофициальные библиотеки Яндекс Музыки и Звука написаны на Python (см. [music-services-ru.md](music-services-ru.md)). Любой другой язык бэкенда потребует портировать их или держать Python-сайдкар, то есть второй рантайм, второй деплой и межсервисный протокол.
- **async I/O.** Приём множества мелких запросов от клиентов и импорт из Last.fm, MusicBrainz и будущих API — классические I/O-bound задачи. asyncio + httpx держат сотни одновременных запросов в одном процессе.
- **Контракт для всех клиентов.** FastAPI строит OpenAPI 3.1 из типов Pydantic, а по нему генерируется типизированный TS-SDK (`@hey-api/openapi-ts` и аналоги) [20]. Один и тот же SDK используют Next.js и будущее React Native-приложение. Нативные Kotlin/Swift получают клиентов из того же OpenAPI (openapi-generator).
- **Данные и рекомендации.** Совместимость вкусов (косинус по топ-артистам), позже коллаборативная фильтрация: numpy, scipy, pandas и библиотеки рекомендаций работают в том же процессе, что и воркеры.
- **Есть прецедент.** ListenBrainz — открытый аналог Last.fm от MetaBrainz — написан на Python (Flask), хранит прослушивания в PostgreSQL/TimescaleDB, использует Redis для now playing и кэша, а фронтенд на React + TypeScript [19]. Это прямое подтверждение, что Python + Postgres тянут сервис скробблинга.
- **Почему FastAPI, а не Django.** Async «из коробки», OpenAPI без дополнительных пакетов, нет HTML-шаблонов, которые не нужны при отдельном Next.js. Django сильнее админкой и встроенным auth, но здесь это менее важно (см. альтернативы).

### arq (очередь и планировщик)

- Воркеры написаны тем же async-кодом, что и API. Не нужно смешивать модели конкурентности, как в связке Celery + async.
- **Уникальный job id** гарантирует одну активную синхронизацию на `account_id` без гонок. Встроенный cron-планировщик выбирает аккаунты по `next_run_at`.
- Брокер — уже имеющийся Valkey (arq говорит на протоколе Redis), отдельный RabbitMQ не нужен.
- Слабое место: проект в maintenance-only (см. «Риски»). Поэтому задачи вызываются через собственный тонкий модуль `jobs` (`enqueue()` + регистрация обработчиков, architecture.md §6.3), а не напрямую через API arq.

### PostgreSQL 16 с секционированием

- **Один движок вместо двух.** Источник истины (пользователи, каталог, прослушивания) и аналитика (топы, стрики, лидерборды) живут в одной БД, поэтому нет ETL и рассинхрона. При допущении в 10 тыс. активных пользователей это ≈110 млн строк в год (оценка из architecture.md §2.3). Для PG с месячными секциями это нормальный объём.
- **Идемпотентность импорта:** `INSERT … ON CONFLICT` по естественному ключу (user, время, трек). Повторный импорт страницы Last.fm и повторная доставка события безопасны.
- **Стрики и периоды** считаются оконными функциями (gaps-and-islands) и `AT TIME ZONE` для локальных дат пользователя.
- **Секции по месяцам** дают дешёвое удаление и архивирование старых данных, а запросы за период трогают только нужные секции. `pg_trgm` нужен для подсказок при слиянии артистов, `citext` — для имён пользователей.
- **Transactional outbox** работает в той же транзакции, что и данные: события для стриков и достижений не теряются.
- Managed PostgreSQL есть у всех рассматриваемых хостеров в РФ, это закрывает бэкапы и PITR.
- **Путь роста:** TimescaleDB (hypertable + continuous aggregates) — расширение того же PG, его уже использует ListenBrainz [19]. ClickHouse — если появятся тяжёлые глобальные чарты по всем пользователям.

### Valkey

- **Почему не Redis.** 20.03.2024 Redis объявил переход с BSD 3-Clause на RSALv2 / SSPLv1 начиная с версии 7.4 [25]; в Redis 8 (01.05.2025) добавлен вариант AGPLv3 [24]. RSALv2 и SSPLv1 — не OSI-лицензии, AGPLv3 — сильный копилефт; для сервиса, который может работать на managed-инфраструктуре хостера, это лишняя юридическая проверка.
- **Что такое Valkey.** Форк Redis 7.2.4, созданный в марте 2024 года под эгидой Linux Foundation; лицензия BSD 3-Clause [26][27]. Протокол и команды совместимы с Redis, поэтому клиенты (redis-py, а через него arq и FSM-хранилище aiogram) подключаются без изменений кода. Совместимость именно нашей связки подтверждаем интеграционными тестами в CI на образе Valkey **(не проверено)**.
- **Два экземпляра:** очередь (`noeviction` + AOF) и кэш (`allkeys-lru`) — у них разные политики вытеснения (architecture.md §7.1).
- Брокер для arq.
- **Глобальный token bucket** на egress-IP: Last.fm может заблокировать ключ за «несколько запросов в секунду постоянно» [11], а MusicBrainz жёстко ограничивает до ≤1 req/s.
- **Sorted sets** — естественная структура для лидербордов (`ZINCRBY` / `ZREVRANGE`).
- Now playing с TTL — так же сделано в ListenBrainz [19].
- Сам Last.fm держит в Redis (≈30 ГБ, Sentinel) сессии API и rate limiting [7]. Тот же паттерн (у нас — на совместимом Valkey), только в масштабе на порядки больше.

### Next.js (TypeScript)

- **SSR публичных профилей** нужен для превью ссылок в мессенджерах и для SEO: «мой топ за год» — главный вирусный сценарий сервиса.
- **Интерактивность:** графики (ECharts/Recharts), сравнение с друзьями, фильтры периодов. Это клиентский React, с которым HTMX справляется хуже.
- **Задел под мобильные приложения.** TypeScript, React, TanStack Query, схемы валидации и сгенерированный API-клиент переиспользуются в Expo/React Native. Через монорепозиторий (Expo поддерживает workspaces [21]) и Solito можно делить даже навигацию и часть UI между Next.js и Expo (Solito 5 совместим с Next.js 16 и Expo SDK 54 [22]). UI-компоненты (Tailwind/shadcn ≠ React Native) переиспользуются **не** полностью — общими будут логика, типы и клиент.
- Фронт работает **только** через публичный API бэкенда (без своих серверных обращений к БД). Поэтому мобильное приложение получает ровно те же возможности.
- Хостинг — Docker-образ `next start` (`output: "standalone"`) на том же сервере в РФ, без Vercel.

### Свои клиенты-скробблеры: расширение и десктоп (этап 1, предложение)

Решение от 04.10.2026: свои клиенты — основной способ подключения, сторонние скробблеры остаются вторым вариантом. Подробно — architecture.md §18.

| Часть | Выбор | Почему и альтернативы |
|---|---|---|
| Расширение | **TypeScript + WebExtension Manifest V3**, сборка **WXT** [29] (одна кодовая база для Chrome/Chromium и Firefox), тесты коннекторов — Vitest на HTML-слепках + ночной Playwright | MV3 обязателен для Chrome Web Store; у Firefox в MV3 фоновые скрипты вместо service worker — WXT сглаживает различия. Основа коннекторов — Web Scrobbler (MIT) [31], см. §18.13 |
| Десктоп | **Tauri 2** [30]: Rust для источников (SMTC — `windows-rs`, MPRIS — `mpris-rs`, macOS — mediaremote-adapter), SQLite, хранилище секретов ОС, встроенный автообновлятор с обязательной подписью | Установщик в разы меньше, чем у Electron (системный WebView вместо своего Chromium). Нативный код для медиасессий нужен в любом случае, а у Rust для этого есть зрелые привязки |
| Десктоп — альтернатива | **Electron**: всё на TypeScript | Если в команде нет Rust; цена — большой установщик и тот же нативный модуль для медиасессий |
| Почему не Qt/C++, как у Last.fm | — | Официальный `lastfm-desktop` написан на Qt/C++ [12][18]; для маленькой команды с TypeScript во фронтенде это второй стек без выгоды |

### Мобильные приложения (позже): что выбирать

| Вариант | Когда подходит |
|---|---|
| **Expo / React Native** (по умолчанию) | Приложение «статистика + профиль + друзья + ручной скроббл». Максимальное переиспользование TS-кода и API-клиента от Next.js, один разработчик ведёт обе платформы |
| **Нативный Kotlin (Android)** | Если нужен фоновый автоскробблинг из любых плееров: это доступ к медиасессиям и уведомлениям, долгоживущий сервис. В RN это всё равно нативный модуль. Официальное Android-приложение Last.fm 2.8 — нативное (AndroidX + Kotlin, см. ниже) |
| **Нативный Swift (iOS)** | iOS не даёт читать воспроизведение в чужих приложениях. Даже приложение Last.fm умеет только «сканировать и отправлять» прослушивания из библиотеки Apple Music [14]. Ценность нативного iOS-клиента ограничена, Expo достаточно |

Бэкенд одинаков для всех вариантов: REST/JSON + OpenAPI, токены, ListenBrainz-совместимый приём скробблов.

### Инфраструктура: Docker Compose, GitHub Actions, Prometheus + Grafana, GlitchTip

- **Docker Compose на хостинге в РФ.** Требование 152-ФЗ о локализации, плюс минимальная операционная сложность: `api`, `worker`, `scheduler`, `web`, `bot`, `postgres`, `valkey` (два экземпляра — очередь и кэш), `caddy`. Kubernetes для одного-двух серверов — избыточно. Переезд в managed k8s у того же провайдера возможен без переписывания, образы те же.
- **GitHub Actions** — CI уже рядом с кодом, GHCR хранит образы. Деплой — `docker compose pull && up -d` по SSH.
- **Prometheus + Grafana.** Ключевые метрики: латентность и доля `5xx` приёма скробблов, «замолчавшие» токены, глубина очередей; для импорта Last.fm — лаг синхронизации на аккаунт, доля ошибок `29` (rate limit) и `17` (приватный профиль), RPS к Last.fm. Алерты — `5xx` приёма > 0,5 % и «лаг > 1 ч у > 5 % аккаунтов с синхронизацией» (architecture.md §7.4).
- **GlitchTip вместо SaaS-Sentry.** Стектрейсы содержат ПДн (username, email), и SaaS за рубежом — это трансграничная передача. GlitchTip принимает Sentry SDK, менять код не нужно.

## Сравнение с альтернативами

Оценка — для команды из 1–2 человек на этапе 1 с перспективой провайдеров РФ и мобильных приложений.

| Критерий | **Выбранный**: Python/FastAPI + arq + PG + Next.js | Full TypeScript: NestJS/Fastify + Drizzle + BullMQ + Next.js | Go (chi/echo + sqlc + River) + SvelteKit | Django + HTMX (+ Celery) |
|---|---|---|---|---|
| Библиотеки Яндекс Музыки / Звука | ✅ нативно (Python) | ❌ порт или Python-сайдкар | ❌ порт или сайдкар | ✅ нативно |
| Async-поллинг внешних API | ✅ asyncio | ✅ Node event loop | ✅ горутины | ⚠️ синхронные воркеры Celery; async в Django частичный |
| Аналитика, рекомендации | ✅ numpy / scipy / pandas | ⚠️ слабее экосистема | ⚠️ слабее экосистема | ✅ та же Python-экосистема |
| Типобезопасный контракт с фронтом и мобилкой | ✅ OpenAPI → TS-SDK | ✅ общие TS-типы без генерации | ✅ OpenAPI → SDK | ⚠️ DRF/django-ninja добавлять отдельно |
| Интерактивные графики и сравнения | ✅ React | ✅ React | ✅ Svelte | ⚠️ HTMX + островки JS |
| Переиспользование кода с React Native | ✅ TS/React, клиент, схемы | ✅ максимум (один язык везде) | ❌ Svelte ≠ React Native | ❌ |
| Скорость MVP | ✅ высокая | ✅ высокая | ⚠️ больше шаблонного кода | ✅ самая высокая (админка, auth) |
| Ресурсы и эксплуатация | ⚠️ 2 рантайма (Python + Node) | ✅ 1 рантайм | ✅ один бинарник, очередь в PG без отдельного брокера | ✅ 1 рантайм (если без SPA) |
| Зрелость очереди | ⚠️ arq — maintenance-only | ✅ BullMQ активен, есть UI | ✅ River активен | ✅ Celery — индустриальный стандарт |

**Почему альтернативы хуже именно для Scrobb:**
- **Full TypeScript** — самый сильный конкурент: один язык, общие типы, лучшая связка с React Native. Проигрывает только из-за провайдеров РФ (Python-библиотеки) и слабее в аналитике. Если от неофициальных API РФ откажутся совсем (push-приём и так основной источник), этот вариант стоит пересмотреть.
- **Go + SvelteKit** — экономичен по ресурсам (так написан Koito), но медленнее итерации для одного разработчика, нет клиентов RU-сервисов, а Svelte не даёт переиспользования с React Native.
- **Django + HTMX** — самый быстрый путь к CRUD-MVP, но графики и сравнения на HTMX сложнее, async в Django и Celery неполный, а для мобильных приложений всё равно нужен отдельный API-слой. Показательно: по свидетельству стороннего разработчика, у Last.fm сайт и API отдаются Django, и это источник техдолга [8] **(не проверено)**.

## Сравнение со стеком Last.fm

> **Важно:** стек Last.fm сложился в 2002–2010-х под десятки миллионов пользователей (≈25 млн в месяц по данным книги «Hadoop: The Definitive Guide» [3], 40 млн скробблов в сутки и пик до 800/с по кейсу HornetQ [9]), с тремя собственными дата-центрами [5] и штатом инженеров. Это наследие эпохи и масштаба, а не образец для команды из 1–2 человек в 2026 году. Полезны паттерны, а не конкретные технологии.

### Что известно о стеке Last.fm

| Слой | Last.fm (период, источник) | Scrobb | Почему иначе |
|---|---|---|---|
| Веб-приложение | **2007:** PHP + Smarty, memcached, балансировщик perlbal с бездисковыми веб-серверами, статика на lighttpd, файлы в MogileFS [1]. **2010:** в вакансиях веб-разработчиков требуют PHP [2]. **Сейчас:** по словам стороннего разработчика, сайт и API на Django [8] **(не проверено)**; косвенно — форки `django`, `django-jinja`, `jinja`, `django-ratelimit`, `pymemcache` в github.com/lastfm [12] | Next.js (SSR) + FastAPI API | Разделение фронта и API с первого дня: тот же API обслуживает мобильные приложения |
| Бэкенд-сервисы | C++ и Java [9]. Стриминг — Java (Tomcat + standalone-стримеры, JMS/HornetQ) [9]. MIR-команда — больше сотни библиотек и сервисов, в основном на C++, часть на Python/Perl/Java [4]; открытые `libmoost` (C++), `musicbrainz-data` (Java/Hibernate) [12] | Один модульный монолит на Python | Стриминга нет, а полиглотная микросервисная система не по силам маленькой команде |
| Основная БД | **PostgreSQL** — основной каталог («main database system runs PostgreSQL») [4][5]. Скробблы в PG-схемах каталога **не** хранились [5]. API-база ключей и приложений — Postgres, в 2020 перенесена в Google Cloud SQL [7] | PostgreSQL 16 | Тот же выбор движка |
| Хранение скробблов и статистика | **Hadoop** с 2006 [3]: скробблы, логи, теги, связи друзей [6]. Кластер 44 узла (2010) [6], больше 60 узлов и +300 ГБ данных в сутки (2011) [10]. Hive / Pig / Dumbo / Avro [6]. На Hadoop считались недельные чарты, похожие артисты, рекомендации, «соседи», индекс поиска [6][13]. Персональные чарты пользователя считались на лету в реальном времени [6] | Секционированная `listens` + rollup'ы в PG; потом TimescaleDB, ClickHouse — только для глобальной аналитики | Сотни миллионов строк помещаются в один PG. Hadoop нужен на десятках миллиардов скробблов |
| Внутренние метрики | Zohmg — хранилище агрегированных временных рядов на **HBase** поверх Hadoop. Cassandra рассматривали, но отвергли из-за слабой документации и сообщества в то время [15][16] | Prometheus + Grafana | Готовый стандарт вместо собственной разработки |
| Кэш, сессии, rate limit | memcached (2007) [1]. **Redis** ≈30 ГБ с Sentinel для сессий API и rate limiting (2020) [7] | Valkey (BSD-форк Redis) | Тот же паттерн, без лицензионных вопросов Redis |
| Поиск | Solr [13] | `pg_trgm`, отдельный поиск — по необходимости | Объём каталога пока мал |
| Инфраструктура | Собственные ДЦ (три площадки, 2011) [10] → **Google Cloud / GKE** (2020), «сложная сервис-ориентированная система» [7] | Docker Compose на VPS в РФ | 152-ФЗ и минимальная эксплуатация |
| Публичный API | `ws.audioscrobbler.com/2.0/`, REST-style, по умолчанию XML, есть JSON (`format=json`) и XML-RPC [11]. **Scrobbling 2.0:** `track.updateNowPlaying` и `track.scrobble`, пакеты до 50 скробблов, подписанные сессии [17]. Старый Submissions Protocol 1.2.1 устарел [17], но в 2020 им ещё пользовались сторонние приложения [7] | REST/JSON + OpenAPI; приём через подмножество ListenBrainz API и Last.fm 2.0-совместимый `/2.0/` | JSON-first и совместимость с уже существующими клиентами (Web Scrobbler, Pano Scrobbler) без своего десктопа |
| Десктоп-скробблер | `lastfm-desktop` — Qt 4.8 + C++ на библиотеке **liblastfm** (Qt/C++), GPL [12][18]. Отдельный Windows-клиент 3.0 на C# (`lastfm-windows-desktop`, 2018) [12] | Свой: Tauri 2, сразу после MVP; плюс расширение браузера в MVP (architecture.md §18) | Свой клиент — основной путь подключения; сторонние скробблеры остаются вторым вариантом |
| Android | Старое открытое приложение `lastfm-android` — Java (последний push в 2015) [12]. Текущее приложение 2.8 (`fm.last.android`) — нативное: AndroidX, Kotlin stdlib и `-ktx`-библиотеки, view binding; признаков Flutter или React Native нет (собственный разбор APK 2.8 со стороннего зеркала APKPure; язык исходников официально не подтверждён) | Позже: Expo/RN или Kotlin | См. раздел о мобильных приложениях |
| iOS | Старое открытое `lastfm-iphone` — Objective-C (2012) [12]. Текущее приложение с 2017 г. (v1.0 — 27.04.2017, v1.9.3 — 18.07.2025), умеет «Scan & submit scrobbles» только для библиотеки Apple Music [14]. Язык **не проверен** | Позже: Expo/RN | — |

### Что берём у Last.fm как паттерн

- PostgreSQL как основная реляционная БД: ровно этот выбор сделал и Last.fm [4][5].
- Хранилище ключ-значение в памяти для сессий, rate limiting и горячих данных: у Last.fm это Redis [7], у нас — совместимый Valkey.
- Разделение «персональные чарты считаются на лету, глобальные — тяжёлым фоновым пересчётом» [6]: у нас это rollup'ы на пользователя плюс фоновые пересчёты лидербордов.
- Правило скроббла (трек > 30 с, проиграна половина или 4 мин) и пакетная отправка закэшированных скробблов [17] — для нашего приёма.
- Чего избегаем: полиглотного зоопарка (PHP + Java + C++ + Python + Perl), собственного Hadoop-кластера и привязки сайта к монолитному фреймворку, из-за которой, по отзывам [8], API сложно развивать.

## Риски и когда пересматривать

| Риск | Сигнал к пересмотру | План |
|---|---|---|
| **arq в режиме maintenance-only** с 18.10.2025: только критичные security-фиксы, новых исправлений не будет [23]. Последний релиз v0.28.0 вышел 16.04.2026 | Баг без фикса, несовместимость с новым Python или redis-py | Задачи уже за тонким интерфейсом. Кандидаты на замену (как в architecture.md §6.3): Dramatiq, SAQ (asyncio, протокол Redis — Valkey) или procrastinate (очередь в PG); запасной — Taskiq. Решить до продакшена |
| **Python и CPU-тяжёлые расчёты** (рекомендации, глобальные чарты) | p95 воркеров растёт, а БД не загружена | Переносить агрегаты в SQL. Тяжёлые батчи — отдельный воркер или сервис (numpy, позже Spark/ClickHouse) |
| **Два рантайма** (Python + Node) | Расходятся типы между фронтом и бэком | Генерация TS-клиента из OpenAPI в CI, проверка «клиент актуален» |
| **Рост PostgreSQL** | > ~100 млн строк в `listens`, медленные rollup'ы | TimescaleDB (continuous aggregates); ClickHouse — для глобальной аналитики |
| **Совместимость Valkey.** Лицензионный риск Redis (RSALv2 / SSPLv1 с 7.4 [25], AGPLv3 в Redis 8 [24]) снят выбором Valkey (BSD 3-Clause [26][27]). Остаётся риск, что форки со временем разойдутся по командам и клиентам | Интеграционный тест на образе Valkey падает; новой версии arq, redis-py или aiogram нужны команды, которых нет в Valkey | Интеграционные тесты на Valkey в CI; клиент `valkey-py` [28] как запасной; замена брокера через интерфейс `jobs` (architecture.md §6.3) |
| **Неофициальные API РФ** ломаются или запрещены ToS | Падает Python-провайдер или приходит претензия | Упор на push-приём (ListenBrainz API). Тогда главный аргумент за Python слабеет, стоит пересмотреть Full TS |
| **Недоступность Last.fm из РФ** (R1) и ToS (R2) | 403 с российского IP, отказ partners@last.fm | Ядро от Last.fm не зависит: основной источник — собственный приём скробблов (architecture.md §3). Для импорта — egress-узел (юридический вопрос) или режим «импорт по запросу пользователя» (§4) |
| **Свои клиенты:** модерация и доступность магазинов расширений из РФ, поломки коннекторов при редизайне сайтов, сертификаты подписи кода (architecture.md §18.11) | Отказ магазина, всплеск `rejected` по коннектору, недоступный сертификат | Самостоятельная раздача подписанного файла для Firefox, kill switch коннекторов, ночные e2e; сторонние скробблеры как запасной путь |
| **Self-hosted наблюдаемость** (GlitchTip, Prometheus) — операционная нагрузка | Время на поддержку больше, чем польза | Managed-мониторинг у хостера в РФ |
| **Python 3.12** — поддержка до октября 2028 (5 лет с релиза в октябре 2023) | 2027–2028 | Плановое обновление до 3.13 / 3.14 |

## Источники

Last.fm:
- [1] M. Ogle (Last.fm), «Scalable LAMP Development for Growing Web Apps», FoWA 2007 (PDF): https://static.last.fm/matt/fowa/workshop.pdf — PHP/Smarty, memcached, perlbal, lighttpd, MogileFS, Hadoop. MySQL на слайдах — из описания типового LAMP, а не утверждение о БД Last.fm.
- [2] Вакансии Last.fm, архив 06.12.2010: https://web.archive.org/web/20101206005325/http://www.last.fm/about/jobs — «deep knowledge of PHP»; data-роль: Python + C++/Java + Hadoop.
- [3] T. White, «Hadoop: The Definitive Guide», гл. «Hadoop Usage at Last.fm»: https://www.oreilly.com/library/view/hadoop-the-definitive/9781449328917/ch16.html — Hadoop с начала 2006, ≈25 млн пользователей в месяц.
- [4] Last.fm Blog, раздел Code (MIR-команда, сборка проектов; «main database system runs PostgreSQL… big data framework… Hadoop»): https://blog.last.fm/index.html?c=Code
- [5] Last.fm Blog, «Artist Artist» (13.10.2010): https://blog.last.fm/2010/10/13/artist-artist — каталог в PostgreSQL, скробблы в Hadoop, Hive.
- [6] «Hadoop at Last.fm» (слайды, 2010): https://www.slideshare.net/slideshow/hadoop-at-lastfm/4646364 — 44 узла, Hive/Pig/Dumbo/Avro; чарты, рекомендации, поиск; персональные чарты на лету.
- [7] Ben XO (Last.fm), «Last.fm To The Cloud: How We Migrated Our API, Live» (12.11.2020): https://benxo.medium.com/last-fm-to-the-cloud-5e1de10ed58a — Postgres → Cloud SQL, Redis ≈30 ГБ + Sentinel, GKE, Scrobble 1/2.0.
- [8] Комментарий автора Open Scrobbler на Hacker News (2023): https://news.ycombinator.com/item?id=35146923 — «They decided to use Django to serve their website and API». Сторонний источник, **не проверено**.
- [9] Кейс «How Last.fm Uses HornetQ for Their Streaming Infrastructure» (2010, перепост оригинала с Javalobby): http://jasper-22.blogspot.com/2010/08/case-study-how-lastfm-uses-hornetq-for.html — бэкенды на C++ и Java, стриминг на Java/Tomcat/JMS; 40 млн скробблов в сутки, пик 800/с.
- [10] Last.fm Blog, «The Adventures of a Data Team Intern» (12.12.2011): https://blog.last.fm/2011/12/12/the-adventures-of-a-data-team-intern — Hadoop больше 60 узлов, +300 ГБ в сутки, три ДЦ, Java/Spring/JDBC.
- [11] Last.fm API — Introduction: https://www.last.fm/api/intro
- [12] GitHub-организация Last.fm: https://github.com/lastfm — `lastfm-desktop`, `liblastfm`, `lastfm-android`, `lastfm-iphone`, `lastfm-windows-desktop`, `libmoost`, форки Django/Jinja (языки — по GitHub API, 03.10.2026).
- [13] Last.fm Blog, «Berlin Buzzwords 2011»: https://blog.last.fm/2011/06/13/berlin-buzzwords-2011 — Solr для поиска, Hadoop для недельных чартов и похожести артистов.
- [14] Last.fm в App Store: https://apps.apple.com/us/app/last-fm/id1188681944
- [15] Zohmg — A Large Scale Data Store for Aggregated Time-series-based Data (работа стажёров data-команды Last.fm, 2009): https://exa.ai/library/publication/mp6rm3hx704; см. также https://blog.last.fm/2009/06/26/tales-of-a-data-team-intern
- [16] R. Jones (сооснователь Last.fm), «Anti-RDBMS: A list of distributed key-value stores» (2009): https://www.metabrew.com/article/anti-rdbms-a-list-of-distributed-key-value-stores
- [17] Last.fm Scrobbling 2.0: https://www.last.fm/api/scrobbling
- [18] README `lastfm-desktop` (Qt ≥ 4.8, liblastfm ≥ 1.0.7): https://github.com/lastfm/lastfm-desktop
- Wikipedia, Last.fm (история, владельцы; независимость с мая 2026): https://en.wikipedia.org/wiki/Last.fm

Стек Scrobb и аналоги:
- [19] ListenBrainz — Architecture: https://listenbrainz.readthedocs.io/en/latest/developers/architecture.html
- [20] FastAPI — Generating SDKs: https://fastapi.tiangolo.com/advanced/generate-clients/
- [21] Expo — Work with monorepos: https://docs.expo.dev/guides/monorepos
- [22] Solito — Compatibility: https://solito.dev/compatibility
- [23] arq — README и issue #510 «Maintenance only mode» (18.10.2025): https://github.com/python-arq/arq/issues/510
- [24] Redis — AGPLv3 (01.05.2025): https://redis.io/blog/agplv3/
- [25] Redis — переход на RSALv2 / SSPLv1 с версии 7.4 (20.03.2024): https://redis.io/blog/redis-adopts-dual-source-available-licensing/
- [26] Linux Foundation — запуск Valkey (28.03.2024): https://www.linuxfoundation.org/press/linux-foundation-launches-open-source-valkey-community
- [27] Valkey — сайт и лицензия BSD 3-Clause: https://valkey.io/, https://github.com/valkey-io/valkey/blob/unstable/COPYING
- [28] valkey-py: https://github.com/valkey-io/valkey-py
- [29] WXT — Next-gen Web Extension Framework (MIT): https://github.com/wxt-dev/wxt
- [30] Tauri 2 и плагин updater (подпись обновлений обязательна): https://github.com/tauri-apps/tauri, https://v2.tauri.app/plugin/updater/
- [31] Web Scrobbler (MIT): https://github.com/web-scrobbler/web-scrobbler
- PostgreSQL 16 — Table Partitioning: https://www.postgresql.org/docs/16/ddl-partitioning.html
- MusicBrainz API — Rate Limiting: https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting
- Внутренние документы: [architecture.md](architecture.md) (§0, §3, §6, §18), [music-services-ru.md](music-services-ru.md)

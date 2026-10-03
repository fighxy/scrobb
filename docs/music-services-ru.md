# Музыкальные сервисы в РФ и способы получения истории прослушиваний

> Статус: черновик, исследование на 03.10.2026. На **этапе 1** основной источник данных — **собственный приём скробблов** Scrobb (ListenBrainz-совместимый API + Last.fm 2.0-совместимый эндпоинт, см. §3 и [architecture.md](architecture.md) §3); Last.fm — дополнительный модуль: разовый импорт истории и опциональная синхронизация (§4 architecture.md). Этот документ — задел для **будущих провайдеров** и выбора клиентов, которые шлют данные в Scrobb.
> Пометка **(не проверено)** — утверждение не подтверждено первоисточником в ходе исследования.

## 0. Главное, что влияет на проект

1. **Last.fm с марта 2026 недоступен из России.** На форуме поддержки Last.fm: «В связи с нынешними ограничениями, мы не можем предоставить доступ к сайту Last.fm в России». Пользователи сообщают, что блокируются и сайт, и эндпоинты скробблинга (HTTP 403), жалобы есть также из Казахстана и Кыргызстана. Обходной путь пользователей — прокси/VPN только для `*.last.fm` и `*.audioscrobbler.com` (Pano Scrobbler добавил SOCKS5).
   Источники: [zona.media](https://zona.media/news/2026/03/24/fm), [DTF](https://dtf.ru/music/4927843-lastfm-zablokiroval-dostup-dlya-polzovateley-iz-rossii), [web-scrobbler#6030](https://github.com/web-scrobbler/web-scrobbler/issues/6030), [discord-music-presence#720](https://github.com/ungive/discord-music-presence/issues/720).
   **Последствие для Scrobb:** у российских пользователей новые скробблы в Last.fm появляются только при включённом обходе блокировки, поэтому Last.fm годится в первую очередь как **источник импорта истории**. Поэтому ядро этапа 1 — собственный push-приём скробблов (см. §3); импорт Last.fm — дополнительный модуль. Кроме того, сервер в РФ может получать 403 от `ws.audioscrobbler.com`: **(не проверено из РФ-IP)**, проверить на первом же деплое.
2. Встроенный скробблинг Яндекс Музыки в Last.fm **отключён**: по сообщениям пользователей, он перестал работать около 25.12.2024, а новые пользователи не могли его включить и раньше ([web-scrobbler#4979](https://github.com/web-scrobbler/web-scrobbler/issues/4979), [#5233](https://github.com/web-scrobbler/web-scrobbler/issues/5233)).

## 1. Доли рынка (по выручке)

Источник: J'son & Partners Consulting в пересказе СМИ ([Forbes 2025](https://www.forbes.ru/forbeslife/565796-vyrucka-muzykal-nyh-strimingov-v-rossii-vyrosla-na-35-v-2025-godu), [Вести/Ведомости](https://www.vesti.ru/ns/vedomosti-vyruchka-muzykalnykh-strimingov-v-rossii-vyrosla-na-35-v-2025-godu), [Forbes 2024](https://www.forbes.ru/biznes/543816-andeks-muzyka-vybilas-v-lidery-po-dohodam-sredi-muzykal-nyh-strimingov-rossii)). Цифры — **доли выручки**, а не доли аудитории.

| Сервис | 2024 | 2025 |
|---|---|---|
| Яндекс Музыка | 50,9% | 54,9% |
| VK Музыка | 26,5% | 20,0% |
| Звук (Сбер) | 10%* | 15,7% |
| Apple Music | 4,5% | — (нет в источнике) |
| МТС Музыка → «КИОН Музыка» | 3,8% | — |
| Spotify | 3,5% | — |

Объём рынка в 2025 году — 53,97 млрд ₽ (+35,2%), из них 99% приходится на подписки. Тройка лидеров занимает 90,6% рынка.
\* Столбец 2024 — по статье за 2024 год. В источниках за 2025 год данные за 2024-й пересмотрены: изменения указаны в п. п. (Яндекс +6,5, VK −5,3, Звук +1,4), откуда пересмотренные доли за 2024 год — ≈48,4%, ≈25,3% и 14,3% соответственно (расчёт по п. п.).

МТС Музыка с 04.12.2025 называется **«КИОН Музыка»** ([CNews](https://www.cnews.ru/news/line/2025-11-25_onlajn-kinoteatr_kion_menyaet), [Lenta](https://lenta.ru/news/2025/11/25/holding-mts-media-rasskazal-podrobnosti-predstoyaschego-rebrendinga/)).

## 2. Сервисы и реальные способы получить прослушивания

| Сервис | Официальный API истории | Неофициально | Браузер (Web Scrobbler) | Прочее | Реалистичный путь |
|---|---|---|---|---|---|
| **Яндекс Музыка** | Нет (публичного API нет) | [MarshalX/yandex-music-api](https://ym.marshal.dev/): `music_history()`; **Ynison** (WebSocket синхронизации плеера) отдаёт текущий трек в реальном времени ([док.](https://ym.marshal.dev/ynison/)). Нужен OAuth-токен пользователя от Яндекс Музыки | Коннектор `yandex-music` есть, но хрупкий: ломался после редизайнов 2025 года, попытка фикса — PR [#6128](https://github.com/web-scrobbler/web-scrobbler/pull/6128) (июль 2026, закрыт без слияния; статус коннектора на 03.10.2026 перепроверить) | Last.fm-интеграция отключена (§0). Есть сторонние боты на cookie `Session_id` ([пример](https://github.com/xKARASb/YaMusicScrobbler)) — небезопасно | Push от Web Scrobbler/Pano Scrobbler в Scrobb; позже, опционально, Ynison-провайдер (риск ToS) |
| **VK Музыка** (vk.com, приложение) | Нет: публичный Audio API закрыт для сторонних приложений 16.12.2016 ([itc.ua](https://itc.ua/news/vkontakte-otklyuchit-publichnyiy-api-audiozapisey-dlya-prilozheniy-storonnih-razrabotchikov/)). `status.get` возвращает трансляцию аудио в статус, только если она включена ([dev.vk](https://dev.vk.ru/ru/method/status.get)) | — | Коннектор `vk` есть. Известная проблема: альбомом указывается имя пользовательского плейлиста ([#5307](https://github.com/web-scrobbler/web-scrobbler/pull/5307)) | Android: Pano Scrobbler по медиа-уведомлениям | Push (Web Scrobbler, Pano Scrobbler) |
| **BOOM** | (не проверено) | — | — | Статус приложения в 2025–2026 годах **не проверен** | Не планировать |
| **Звук** (zvuk.com) | Нет | [zvuk-music](https://pypi.org/project/zvuk-music/): есть `listening-history`, токен берётся из `zvuk.com/api/tiny/profile`, есть антибот-защита | Коннектор `zvuk` есть, после редизайна чинился ([#5236](https://github.com/web-scrobbler/web-scrobbler/issues/5236), закрыт) | — | Push; позже — провайдер истории (риск ToS) |
| **КИОН Музыка** (ex-МТС) | (не проверено) | (не проверено) | Коннектор `mts-music` для `music.mts.ru` есть; работает ли он после ребрендинга — не проверено | — | Push |
| **Spotify** | Да: `GET /me/player/recently-played`, **не больше 50 последних** треков ([docs](https://developer.spotify.com/documentation/web-api/reference/get-recently-played)), нужен OAuth | — | Коннектор есть | Ушёл из РФ в 2022 году. Выгрузка «extended streaming history» через privacy-запрос (не проверено в этом исследовании) | OAuth-поллинг + импорт выгрузки |
| **Apple Music** | Полной истории в API нет (не проверено) | — | Коннектор `apple-music` есть | Оплата российскими картами недоступна (J'son, Forbes 2024) | Push; SMTC/Now Playing |
| **YouTube Music** | Нет (не проверено) | `ytmusicapi.get_history` (не проверено) | Коннектор `youtube-music` есть | Google Takeout (не проверено) | Push |
| **Локальные плееры** | — | — | — | Windows SMTC ([MS docs](https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssessionmanager)); Linux MPRIS ([rescrobbled](https://github.com/InputUsername/rescrobbled)); macOS с 15.4 закрыл MediaRemote для сторонних приложений, обход — [mediaremote-adapter](https://github.com/ungive/mediaremote-adapter). AIMP/foobar2000/VLC: встроенные или плагинные Last.fm-скробблеры (не проверено, можно ли сменить URL) | Существующие скробблеры (rescrobbled, foobar2000 + foo_listenbrainz2, Pano Scrobbler на Windows/Linux) через ListenBrainz-совместимый API; **своё десктоп-приложение** на тех же медиасессиях — этап 1, сразу после MVP ([architecture.md](architecture.md), §18.6) |

Набор коннекторов Web Scrobbler сверен по `src/core/connectors.ts` в master, релиз v3.23.1 от 19.09.2026.

## 3. Push-совместимость: пусть существующие скробблеры шлют данные в Scrobb

| Клиент | Свой URL ListenBrainz | Источник |
|---|---|---|
| Web Scrobbler (браузер) | Да. Указывать полный путь `…/1/submit-listens` | [#5795](https://github.com/web-scrobbler/web-scrobbler/issues/5795), [исходник](https://github.com/web-scrobbler/web-scrobbler/blob/master/src/core/scrobbler/listenbrainz/listenbrainz-scrobbler.ts) |
| Pano Scrobbler (Android/Windows/Linux) | Да («Custom ListenBrainz»; также GNU FM — Last.fm 2.0-совместимый сервер), есть SOCKS5 | [блог](https://wenkdth.org/posts/maloja-scrobbling/), [репо](https://github.com/kawaiidango/pano-scrobbler) |
| foobar2000 + foo_listenbrainz2 | Да (свой URL с `/submit-listens` на конце) | [README](https://github.com/phw/foo_listenbrainz2) |
| Navidrome, rescrobbled | Да (Navidrome — `ListenBrainz.BaseURL`, rescrobbled — `[[listenbrainz]]` с `url`) | [блог](https://wenkdth.org/posts/maloja-scrobbling/), [rescrobbled](https://github.com/InputUsername/rescrobbled) |

Так уже сделано у self-hosted аналогов: Maloja (базовый URL `/apis/listenbrainz` и GNU FM — `/apis/audioscrobbler`, [документация](https://github.com/krateng/maloja/blob/master/API.md)) и Koito. Формат: [ListenBrainz JSON](https://listenbrainz.readthedocs.io/en/latest/users/json.html).
**Вывод:** ListenBrainz-совместимый эндпоинт (плюс Last.fm 2.0-совместимый `/2.0/` для клиентов, умеющих только GNU FM) даёт поддержку Яндекс Музыки, VK, Звука и КИОН через Web Scrobbler, а Android-приложений — через Pano Scrobbler, без неофициальных API. Основной путь при этом — **свои клиенты** (расширение с коннекторами для тех же сайтов и десктоп-приложение, [architecture.md](architecture.md), §18), а Web Scrobbler — и поддерживаемый сторонний клиент, и основа для коннекторов (MIT, §18.13).

## 4. Риски неофициальных подходов

- Неофициальные API Яндекса и Звука требуют **токена или cookie сессии пользователя**: это полный доступ к его аккаунту. Хранение таких токенов — повышенная ответственность (152-ФЗ, утечки), к тому же это, вероятно, нарушает пользовательские соглашения сервисов (тексты ToS не анализировались, **не проверено**).
- DOM-коннекторы ломаются при каждом редизайне. Поддерживает их сообщество Web Scrobbler, а не мы.
- У API могут быть антибот-защиты (у Звука — `BotDetectedError`, у Яндекса — капча).

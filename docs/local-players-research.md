# Локальные кроссплатформенные музыкальные плееры: обзор и стоит ли писать свой

> Статус: **исследование на 23.10.2026**. Версии и даты сверены по официальным сайтам и страницам проектов на GitHub, где это удалось; остальное — по новостным сайтам и каталогам пакетов. Пометка **(не проверено)** — факт не подтверждён первоисточником.
> Связь с проектом: Scrobb не плеер, а сервис скробблинга. Локальные плееры для нас — **источник прослушиваний** (десктоп-клиент по медиасессиям, [architecture.md](architecture.md) §18.6; сторонние скробблеры, §3.3). Поэтому вопрос «писать ли свой плеер» разобран с позиции Scrobb (§6).

## 0. Коротко

1. **Рынок насыщен.** Зрелых бесплатных плееров много: foobar2000, AIMP и MusicBee для Windows; Strawberry, fooyin, DeaDBeeF, Audacious, Quod Libet и Tauon с открытым кодом. Базовые функции (gapless, ReplayGain, редактор тегов, умные плейлисты, плагины) есть почти у всех зрелых плееров.
2. **Действительно кроссплатформенного лидера нет.** foobar2000 работает на Windows, macOS и мобильных, но не на Linux. AIMP6 работает на Windows и нативно на Linux, но пока в бете и без macOS. Strawberry есть на трёх десктопных ОС, но сборки для Windows и macOS выдаются только спонсорам на Patreon. Rune есть на шести платформах, но он молодой и платный.
3. **Главные пробелы рынка:** история прослушиваний и статистика (плееры хранят счётчики, а не историю), синхронизация состояния между компьютером и телефоном, рекомендации по своей коллекции, автоматическая чистка метаданных (в том числе кириллицы в тегах). Первые три — ровно та зона, где работает Scrobb.
4. **Рекомендация: свой плеер сейчас не писать.** Полноценный плеер — это годы работы, соперничество с бесплатными продуктами двадцатилетней зрелости и слабая монетизация. Десктоп-клиент Scrobb по медиасессиям (§18.6) покрывает почти все локальные плееры за малую долю этих усилий. Вместо плеера — интеграции, импорт статистики из баз плееров и страницы «Как подключить плеер» (§5.2). Вернуться к вопросу по метрикам после запуска десктоп-клиента (§6.4).

## 1. Что сравниваем

- **Локальный плеер** — воспроизводит файлы с диска и ведёт свою библиотеку. Клиенты стриминговых сервисов не рассматриваем.
- **Кроссплатформенный** — минимум две из трёх десктопных ОС (Windows, macOS, Linux). MusicBee и AIMP5 работают только на Windows, но включены как эталон функций и из-за популярности в РФ.
- **Смежные категории** (кратко, §2.3): клиенты self-hosted серверов (Navidrome, Jellyfin) и мобильные плееры. Они отбирают часть аудитории у «чистых» локальных плееров.

## 2. Обзор плееров

### 2.1 Зрелые плееры с богатыми функциями

| Плеер | Платформы | Лицензия и модель | Стек | Актуальная версия | Сильные стороны | Слабые стороны |
|---|---|---|---|---|---|---|
| **foobar2000** | Windows, macOS, Android, iOS; Linux — только Windows-версия под Wine | Freeware, закрытый код | C++ | 2.26 для Windows и macOS (01.10.2026, [сайт](https://www.foobar2000.org/)) | Почти все форматы, gapless, ReplayGain, мощный редактор тегов, DSP, рип CD и конвертер, радио (Radio Browser), компоненты, без телеметрии | Сложная настройка, аскетичный интерфейс «из коробки». Версия для macOS заметно беднее Windows-версии: компоненты для macOS появились только в 2025 году, пользователи жалуются на нехватку функций ([Hydrogenaudio](https://hydrogenaudio.org/index.php/topic,122517.0.html), [Rosettacheck](https://rosettacheck.com/apps/com.foobar2000.mac)). Нативного Linux нет |
| **AIMP** | Windows (AIMP5; AIMP6 в бете, есть сборка для Windows on ARM), **Linux нативно** (AIMP6 beta с 16.03.2026), Android 4.50 (24.08.2026). macOS нет | Freeware, закрытый код, российский разработчик (Артём Измайлов) | Delphi (Object Pascal); под Linux — GTK2 + Cairo | AIMP5 5.40; AIMP6 — бета, релиз запланирован на конец 2026 года ([блог](https://aimp.ru/blogs/?m=2026), [анонс беты](https://www.aimp.ru/?newsid=258)) | Удобный интерфейс, скины, встроенный скробблер Last.fm, конвертер, редактор тегов, плагины. Android-версия развита: умные плейлисты, Android Auto, настраиваемый порог скроббла | Linux-сборка без плагинов Winamp/AIMP5, DSP/VST и TAK ([анонс беты](https://www.aimp.ru/?newsid=258)); бета «может содержать много ошибок». Нет macOS и iOS |
| **MusicBee** | Только Windows (macOS/Linux — Wine или виртуалка) | Freeware, закрытый код | C# / .NET | 3.6.x ([VideoHelp](https://www.videohelp.com/software/MusicBee)); дата релиза **(не проверено)** | Лучшее на Windows управление библиотекой: авто-теги, Auto DJ, синхронизация с устройствами, скины, плагины, Last.fm | Только Windows; один разработчик |
| **Strawberry** | Windows, macOS, Linux | GPL-3.0, но **готовые сборки для Windows, macOS и AppImage — только спонсорам на Patreon** (от $7 в месяц для новых); для Linux — бесплатно через репозитории дистрибутивов | C++ / Qt 6, GStreamer | 1.2.31 (02.10.2026, [GitHub](https://github.com/strawberrymusicplayer/strawberry/releases)) | Библиотека, редактор тегов с MusicBrainz, тексты песен и обложки из многих источников, умные плейлисты, Subsonic, синхронизация с USB/MTP/iPod, скробблинг Last.fm и ListenBrainz ([сайт](https://www.strawberrymusicplayer.org/)) | Интерфейс эпохи Clementine. Функции неравномерны по ОС: bit-perfect только на Linux, синхронизации с устройствами и CD нет на Windows. Сборки за Patreon: оплата Patreon российскими картами, вероятно, невозможна **(не проверено на 2026)** |
| **fooyin** | Linux, Windows (с 0.12, 2026); macOS — «скоро» | GPL-3.0 | C++ / Qt | 0.12.x (2026, [Neowin](https://www.neowin.net/amp/fooyin-0126/), [LinuxToday](https://www.linuxtoday.com/blog/fooyin-v0-12-0-adds-audio-conversion-and-new-visualisations/)) | «Открытый foobar2000»: раскладка из виджетов, FooScript, DSP-цепочки, gapless и bit-perfect, CUE, AccurateRip, конвертер, редактор тегов с MusicBrainz, синхронизация текстов, **скробблинг в Last.fm, Libre.fm, ListenBrainz и «custom»** ([README](https://github.com/fooyin/fooyin)) | Молодой проект (2,6 тыс. звёзд), нет macOS, на Windows — с 2026 года |

### 2.2 Лёгкие и «красивые» плееры

| Плеер | Платформы | Лицензия | Стек | Версия | Сильные стороны | Слабые стороны |
|---|---|---|---|---|---|---|
| **DeaDBeeF** | Linux, Windows, macOS | zlib (ядро) + GPL/LGPL (плагины) | C | 1.10.x (1.10.0 — 25.03.2025, [Wikipedia](https://en.wikipedia.org/wiki/DeaDBeeF)) | Лёгкий, много форматов (в том числе chiptune), CUE, плагины; в 1.10 появилась медиатека и undo/redo плейлистов | Windows-сборка без обложек, форматов FFmpeg, CD и конвертера; сборка для macOS не подписана, Gatekeeper её блокирует ([загрузка](https://deadbeef.sourceforge.io/download.html)) |
| **Audacious** | Linux, Windows, macOS | BSD-2-Clause | C/C++, Qt 6 и GTK | 4.6 (31.05.2026; добавлен плагин Now Playing для macOS, [новости](https://www.audacious-media-player.org/news/64-audacious-4-6-released)) | Очень лёгкий, скины Winamp, тексты песен | Ориентирован на плейлисты, а не на библиотеку |
| **Quod Libet** | Linux (основная), Windows, macOS | GPL-2.0+ | Python / GTK | 4.7.1 (31.05.2025, [Wikipedia](https://en.wikipedia.org/wiki/Quod_Libet_(software))) | Язык запросов к библиотеке, Ex Falso для тегов, плагины | Редкие релизы (последний — больше года назад), интерфейс старомоден |
| **Tauon** | Linux, Windows, macOS (подписанный DMG) | GPL-3.0 | Python + C | v11.1.1 (07.2026), есть и v12.0.0 — дата **(не проверено)** ([сайт](https://tauonmusicbox.rocks/manual/installing/)) | Быстрый интерфейс, gapless, CUE, визуализации Milkdrop, стриминг с Plex/Jellyfin/Airsonic, импорт архивов в один клик ([README](https://github.com/Taiko2k/Tauon)) | Непривычный интерфейс, маленькая команда |
| **Dopamine** | Windows, Linux, macOS | GPL-3.0 | Electron + Angular + TypeScript | 3.0.x ([Neowin](https://www.neowin.net/amp/dopamine-303/)) | Простой и красивый интерфейс, gapless, умные плейлисты, кроссфейд, Last.fm, тексты, Discord | Electron (размер, память), мало функций для «продвинутых» |
| **Museeks** | Windows, macOS, Linux | **MIT** | **Tauri 2 + Rust + React** | 0.23.x (пакет AUR от 25.02.2026) | Крошечный: при переходе с Electron на Tauri сборка уменьшилась примерно с 201 до 8 МБ ([VideoHelp](https://videohelp.com/software/Museeks/version-history)); простой | Сознательно минималистичен: «цель — не конкурировать с более полными плеерами» ([README](https://github.com/martpie/museeks)) |
| **Harmonoid** | Windows, Linux, Android | PolyForm Strict 1.0.0 (исходники открыты, но это не open source); платная подписка Plus | Flutter / Dart + libmpv | 0.3.x; активность в 2026 году **(не проверено)** | Gapless, смена скорости и тона, синхронные тексты, MPRIS ([It's FOSS](https://itsfoss.com/harmonoid/)) | Лицензия запрещает переиспользование, статус разработки неясен |
| **Rune** | Windows, macOS, Linux, SteamOS, Android, iOS | MPL-2.0; готовые сборки — платно через Steam, Microsoft Store, App Store (от $10) | Rust + Flutter | Активен, около 1 тыс. звёзд ([GitHub](https://github.com/Losses/rune)) | **Анализ аудио и рекомендации по своей коллекции**, динамические «миксы» по правилам («похоже на этот альбом», «топ-100 по прослушиваниям») | Маленькое сообщество, платные сборки |

Устаревшие и непрофильные:
- **Clementine** (Windows, macOS, Linux; GPL-3.0) — выходят только снапшоты 1.4.1 (последний — 25.04.2026, [Wikipedia](https://en.wikipedia.org/wiki/Clementine_(software))). Его развивающийся форк — Strawberry.
- **VLC** — универсальный медиаплеер на всех платформах, но не менеджер музыкальной библиотеки.
- **Nuclear** (AGPL-3.0, 18,8 тыс. звёзд, Tauri + React) — агрегатор стриминга, а не локальный плеер: «ищи музыку и стримь из любого источника» ([GitHub](https://github.com/nukeop/nuclear)).

### 2.3 Смежные категории

- **Self-hosted серверы и их клиенты.** Navidrome за 2026 год добавил WebAssembly-плагины и Instant Mix (0.60), аудиосходство через расширение OpenSubsonic `sonicSimilarity` (0.62), пословные синхронные тексты (0.63) и экспериментальный Jellyfin Music API (0.64) ([AlternativeTo](https://alternativeto.net/news/2026/9/navidrome-0-64-brings-jellyfin-music-api-faster-artwork-delivery-and-performance-gains/), [Open Collective](https://opencollective.com/navidrome/updates/navidrome-v0-63-word-by-word-synchronized-lyrics-smarter-search-and-speed-improvements)). Клиент Feishin (GPL-3.0, Electron, около 10,2 тыс. звёзд) работает с Navidrome, Jellyfin и OpenSubsonic-серверами, с бэкендами воспроизведения MPV и веб ([GitHub](https://github.com/jeffvli/feishin)). Это путь «своя коллекция на всех устройствах», но он требует сервера и навыков его администрирования.
- **Мобильные плееры.** На Android — AIMP (развит, см. выше), а также Poweramp и Musicolet (в этом исследовании не проверялись). На iOS — foobar2000 mobile, VOX, Doppler ([VOX](https://vox.rocks/blog/ios-flac-player/)). Связки «десктоп + телефон» с общей библиотекой и историей у локальных плееров практически нет (§4, п. 2).

## 3. Кор-функции

### 3.1 Базовый набор (без него плеер не воспринимают всерьёз)

| Группа | Функции |
|---|---|
| Воспроизведение | MP3, AAC/M4A, FLAC, ALAC, Ogg Vorbis/Opus, WAV/AIFF, APE, WavPack; **gapless**; **ReplayGain**; кроссфейд; эквалайзер; выбор устройства вывода |
| Библиотека | Сканирование папок и слежение за изменениями, просмотр по артистам, альбомам и жанрам, быстрый поиск, очередь, плейлисты M3U/M3U8, умные (авто-) плейлисты, рейтинги и счётчики прослушиваний |
| Метаданные | Редактор тегов (в том числе пакетный), обложки, тексты песен (всё чаще синхронные LRC) |
| Интеграция с ОС | Медиасессии (SMTC в Windows, MPRIS в Linux, Now Playing в macOS), медиаклавиши, уведомления |
| Скробблинг | Last.fm почти у всех; ListenBrainz — у новых и открытых плееров |

### 3.2 Продвинутые функции (отличают «серьёзные» плееры)

- **Звук для аудиофилов:** bit-perfect, эксклюзивный режим (WASAPI exclusive, ASIO, CoreAudio hog mode), DSD, ресемплинг, DSP-цепочки, VST/Audio Units.
- **Работа с метаданными:** поиск в MusicBrainz и Discogs, AcoustID, переименование и перемещение файлов по шаблону, поиск дублей.
- **Форматы и носители:** CUE-образы, рип CD с AccurateRip, конвертер, архивы, трекерная музыка и chiptune.
- **Расширяемость:** плагины и компоненты (foobar2000, AIMP, MusicBee, DeaDBeeF, fooyin), скриптовые языки (title formatting у foobar2000, FooScript у fooyin, язык запросов у Quod Libet), скины, раскладки из виджетов.
- **Сеть:** интернет-радио, стриминг с Subsonic/Jellyfin/Plex, UPnP/DLNA, Chromecast/AirPlay, удалённое управление.
- **Устройства:** синхронизация с USB/MTP/iPod (MusicBee, Strawberry).
- **Прочее:** визуализации (Milkdrop у Tauon), Discord Rich Presence, Auto DJ, таймер сна.

### 3.3 Матрица ключевых функций

✓ — есть, ~ — частично или через плагин, — — нет, ? — не проверено.

| Функция | foobar2000 | AIMP | MusicBee | Strawberry | fooyin | DeaDBeeF | Tauon | Dopamine |
|---|---|---|---|---|---|---|---|---|
| Windows / macOS / Linux | ✓ / ~ / Wine | ✓ / — / ✓ (бета) | ✓ / — / — | ✓ / ✓ / ✓ | ✓ / — / ✓ | ✓ / ✓ / ✓ | ✓ / ✓ / ✓ | ✓ / ✓ / ✓ |
| Мобильная версия | ✓ (Android, iOS) | ✓ (Android) | — | — | — | — | ? | — |
| Открытый код | — | — | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| Gapless | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Редактор тегов с MusicBrainz | ~ (компоненты) | ? | ✓ | ✓ | ✓ | ? | ? | ? |
| Умные плейлисты | ✓ | ✓ | ✓ | ✓ | ✓ | ? | ? | ✓ |
| Плагины | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | — |
| Стриминг с self-hosted сервера | ~ | ? | ? | ✓ (Subsonic) | ~ (URL-потоки) | ? | ✓ (Plex, Jellyfin, Airsonic) | — |
| Скробблинг Last.fm | ~ (компонент) | ✓ | ✓ | ✓ | ✓ | ✓ (плагин) | ✓ | ✓ |
| **ListenBrainz со своим адресом сервера** | ✓ (foo_listenbrainz2) | ? | ? | ? | ✓ («custom») | ? | ? | ? |

Последняя строка важна для Scrobb: только плееры со своим адресом ListenBrainz (или Last.fm 2.0) могут слать прослушивания в Scrobb напрямую, без десктоп-клиента ([architecture.md](architecture.md) §3.3). Для остальных отметок «?» нужна проверка в настройках (§5.2, п. 2).

## 4. Сильные и слабые стороны рынка в целом

**Сильные стороны:**
- Движки воспроизведения зрелые: форматы, gapless, ReplayGain и DSP решены давно и хорошо.
- Много бесплатных вариантов, в том числе с открытым кодом. На Windows foobar2000, AIMP и MusicBee закрывают почти любые потребности.
- Живое сообщество плагинов и компонентов.
- Нет рекламы и телеметрии: у foobar2000 это явное обещание.

**Слабые стороны и пробелы:**
1. **Фрагментация по ОС.** Лучшие по функциям плееры — Windows-first. Хуже всего на macOS: MusicBee и AIMP там нет, а foobar2000 для macOS беднее Windows-версии. На Linux хорошие открытые плееры, но с менее отполированным интерфейсом.
2. **Нет непрерывности «компьютер ↔ телефон».** Рейтинги, счётчики, плейлисты и позиция воспроизведения не синхронизируются. У foobar2000 и AIMP есть мобильные версии, но переносить плейлисты приходится вручную (у AIMP для этого есть отдельная инструкция, [блог](https://aimp.ru/blogs/?m=2026)). Единственный полноценный путь — свой сервер (Navidrome, Jellyfin), а это порог для большинства.
3. **История прослушиваний — чёрный ящик.** Плееры хранят счётчик и дату последнего прослушивания, но не историю. Исключение — компонент foo_enhanced_playcount для foobar2000: он пишет время каждого прослушивания и умеет подтягивать время скробблов из Last.fm ([страница компонента](https://foobar2000.org/components/view/foo_enhanced_playcount)). Статистику, отчёты и «итоги года» плееры не строят — за этим идут в Last.fm, а он в РФ заблокирован ([music-services-ru.md](music-services-ru.md) §0).
4. **Нет открытий по своей коллекции.** Рекомендации и «радио» по локальной библиотеке есть только у единичных продуктов: Rune (анализ аудио), Navidrome (Instant Mix, `sonicSimilarity`), Plexamp. У классических плееров — разве что Auto DJ (MusicBee) и случайный порядок.
5. **Чистка метаданных — ручная работа.** Теги правят в отдельных программах (MusicBrainz Picard) или во встроенных редакторах. Для российских коллекций есть особая боль — «кракозябры»: теги в кодировке cp1251, записанные без указания кодировки (ID3v1 и старые ID3v2), а также один артист под разными написаниями («Кино» и «Kino»). Это же мешает и нормализации в Scrobb (§5 architecture.md).
6. **Мощь или красота.** Мощные плееры выглядят устаревшими (foobar2000 «из коробки», Quod Libet, Strawberry), а красивые неглубокие (Dopamine, Museeks).
7. **Устойчивость проектов.** Многие ключевые плееры держатся на одном разработчике: foobar2000, AIMP, MusicBee, Strawberry. Монетизация слабая: донаты, сборки за Patreon (Strawberry), платные сборки в магазинах (Rune), подписка (Harmonoid).
8. **Распространение.** Неподписанные сборки (DeaDBeeF для macOS), сборки за Patreon (Strawberry), магазины приложений — у каждого способа свои барьеры, а для пользователей из РФ добавляются проблемы с оплатой.
9. **Скробблинг на свой сервер — лотерея.** Свой адрес ListenBrainz умеют единицы (§3.3), остальные знают только Last.fm.

## 5. Что можно добавить

### 5.1 Идеи для плеера (по ценности для пользователя)

| # | Идея | Почему ценно | Кто уже делает |
|---|---|---|---|
| 1 | **История как основная сущность:** каждое прослушивание со временем, пропуски, частичные прослушивания, источник; экспорт в формате ListenBrainz JSON | Основа для статистики, рекомендаций и переноса между плеерами | Частично foo_enhanced_playcount |
| 2 | **Статистика и «итоги года» офлайн** по своей коллекции | Last.fm недоступен, у стримингов есть итоги, у локальных плееров — нет | Нет |
| 3 | **Коллекция ↔ история:** «40% альбомов ни разу не слушал», «забытые любимые», «что давно не играло» | Возвращает ценность большой коллекции | Нет (частично умные плейлисты) |
| 4 | **Синхронизация состояния без хранения файлов:** рейтинги, счётчики, плейлисты, позиция — через облако, файлы остаются у пользователя | Закрывает пробел «компьютер ↔ телефон» без своего сервера | Нет |
| 5 | **Рекомендации по своей коллекции:** анализ аудио, «радио от трека», учёт истории | Открытия без стриминга | Rune, Navidrome, Plexamp |
| 6 | **Автопилот метаданных:** AcoustID-отпечатки, MusicBrainz, обложки, дубли, **исправление кодировок кириллицы**, сведение написаний артистов | Самая нудная работа с коллекцией | Частично Strawberry, fooyin, MusicBee, Picard |
| 7 | Синхронные тексты (LRC, пословные) | Становится стандартом | Navidrome 0.63, Harmonoid, Feishin, fooyin |
| 8 | Социальное: «что сейчас слушают друзья», в том числе из локальных файлов | Локальное прослушивание сейчас невидимо | Last.fm (заблокирован), **Scrobb** |

### 5.2 Что из этого делать Scrobb без своего плеера

Пункты 1–3 и 8 — это и есть Scrobb, только на сервере. Конкретные шаги, от дешёвых к дорогим:

1. **Десктоп-клиент по медиасессиям** — уже в плане ([architecture.md](architecture.md) §18.6, §18.14). Он покрывает любой плеер, который публикует SMTC/MPRIS/Now Playing. Добавить в план прототипа **матрицу проверки плееров**: foobar2000, AIMP5/AIMP6, MusicBee, Strawberry, fooyin, Audacious, Quod Libet, DeaDBeeF, Tauon, Dopamine, VLC. Проверить: публикует ли плеер сессию, полные ли метаданные, приходит ли длительность и позиция. Публикуют ли сессии foobar2000, AIMP и MusicBee без плагинов — **(не проверено)**.
2. **Страницы «Как подключить плеер»** ([architecture.md](architecture.md) §18.11, `/apps`) для плееров со своим адресом сервера: foobar2000 + foo_listenbrainz2, fooyin («custom»), Navidrome, rescrobbled. Проверить и дописать по результатам: Strawberry, DeaDBeeF, Quod Libet, Tauon, AIMP, MusicBee (строка «ListenBrainz со своим адресом» в §3.3).
3. **PR в открытые плееры:** где нет поля «адрес сервера ListenBrainz», предложить его upstream (Strawberry, Tauon, Dopamine — если подтвердится, что поля нет). Дёшево, полезно сообществу (Maloja, Koito и другие self-hosted сервисы получат то же самое) и снимает зависимость от десктоп-клиента.
4. **Импорт статистики из баз локальных плееров** — новая функция рядом с импортом Last.fm ([architecture.md](architecture.md) §4). Источники-кандидаты:

   | Источник | Что есть | Формат |
   |---|---|---|
   | foobar2000 + foo_enhanced_playcount | Время **каждого** прослушивания | Поля компонента, читаются скриптом или экспортом **(способ выгрузки не проверен)** |
   | foobar2000 + foo_playcount | Счётчик, первое и последнее прослушивание | Поля библиотеки |
   | iTunes / Музыка (macOS) | Play Count, Play Date UTC | `Library.xml` (экспорт) |
   | Strawberry | Счётчик, последнее прослушивание | SQLite **(схема не проверена)** |
   | Quod Libet | `~#playcount`, `~#lastplayed` | Файл библиотеки **(формат не проверен)** |
   | MusicBee, AIMP | Счётчики **(не проверено)** | **(не проверено)** |

   Важное ограничение для модели данных: счётчик без времени нельзя положить в таблицу прослушиваний — у каждой записи там есть `listened_at`. Такие данные нужно хранить отдельно, как «исторический агрегат». Он может участвовать в топах «за всё время», но не в стриках, отчётах по периодам и лидербордах. Полную историю из foo_enhanced_playcount можно импортировать как обычные прослушивания. Решение — в открытые вопросы (§16 architecture.md).
5. **Плагин для AIMP** — только если проверка (п. 1) покажет, что AIMP не публикует медиасессию с полными метаданными. AIMP — российский плеер с большой аудиторией в РФ (доля **не проверена**), поэтому это канал, специфичный для нашего рынка.
6. **Нормализация кириллицы на сервере** (п. 6 из §5.1): исправление кракозябр cp1251 и сведение «Кино»/«Kino» — в слое сопоставления ([architecture.md](architecture.md) §5.3). Это ценно для всех источников, не только для локальных плееров.

## 6. Стоит ли писать свой плеер

### 6.1 Из чего состоит плеер и сколько это стоит

Оценка трудозатрат — грубая, для одного опытного разработчика, **не измерено**.

| Компонент | Что внутри | Сложность |
|---|---|---|
| Декодирование | FFmpeg / libmpv / GStreamer или чистый Rust (Symphonia). Symphonia не во всех форматах поддерживает gapless: по документации 0.5, в контейнерах MP4 и MKV и для AAC-LC его нет ([docs.rs](https://docs.rs/symphonia/^0.5)) | Средняя: готовые библиотеки, но много крайних случаев |
| Вывод звука | WASAPI (в том числе exclusive), CoreAudio, ALSA/PulseAudio/PipeWire; смена устройства на лету; ресемплинг | Высокая: основной источник багов от пользователей |
| Gapless, ReplayGain, кроссфейд | Предзагрузка следующего трека, анализ громкости | Средняя |
| Библиотека | Сканер, чтение тегов (TagLib, lofty), слежение за папками, SQLite + FTS5, виртуализированные списки на 100 тыс.+ треков | Высокая |
| Метаданные | Редактор тегов, обложки, тексты, MusicBrainz | Средняя |
| Интеграция с ОС | SMTC, MPRIS, Now Playing, медиаклавиши, трей | Средняя (частично уже нужна десктоп-клиенту) |
| Распространение | Подпись кода, нотаризация, автообновление, пакеты для Linux | Уже решаем для десктоп-клиента ([architecture.md](architecture.md) §18.10–§18.11) |
| Мобильные версии | Отдельные приложения или Flutter | Очень высокая |

- **MVP уровня Museeks или Dopamine** (воспроизведение, простая библиотека, плейлисты): 3–6 человеко-месяцев.
- **Уровень Strawberry или fooyin** (теги, умные плейлисты, плагины, DSP): 1,5–3 человеко-года.
- **Уровень foobar2000** — больше 20 лет развития.

### 6.2 Аргументы «за»

- **Свой плеер = свои данные.** Не нужно угадывать по медиасессиям: точное время звучания, пропуски, частичные прослушивания (§5.1, п. 1). Особенно ценно на macOS, где чтение Now Playing закрыто с 15.4 и держится на обходе ([architecture.md](architecture.md) §18.6).
- **Пробел на рынке реален:** «кроссплатформенный MusicBee с историей, статистикой и синхронизацией» не существует (§4).
- **Есть быстрый старт:** Museeks — под **MIT** и на **том же стеке, что десктоп-клиент Scrobb** (Tauri 2 + Rust + React, [architecture.md](architecture.md) §18.12). Прототип из форка — недели, а не месяцы.
- Позиционирование: «плеер, который помнит всё, что вы слушали».

### 6.3 Аргументы «против» (перевешивают)

- **Конкуренция с бесплатными и зрелыми продуктами.** Пользователи локальных плееров консервативны и годами настраивали foobar2000, AIMP или MusicBee. Переманить их новым плеером без десятков привычных функций почти невозможно, а Scrobb нужно, чтобы они **скробблили**, а не меняли плеер.
- **Ниша узкая и не измерена.** 99% выручки музыкального рынка РФ — подписки на стриминг ([music-services-ru.md](music-services-ru.md) §1). Какая доля слушателей пользуется локальными файлами — **не проверено**; подтверждённого тренда «возврата к своим файлам» в 2026 году поиск не нашёл. Есть лишь отдельные сигналы из отраслевых новостей: подорожание подписок Spotify и Apple Music в 2026 году, растущая доля AI-музыки среди загрузок на стриминги, запрет AI-треков на Bandcamp ([RouteNote, июль 2026](https://routenote.com/blog/music-industry-news-july-2026), [RouteNote, январь 2026](https://routenote.com/blog/music-industry-news-january-2026/)); первоисточники этих цифр **не проверены**.
- **Монетизация слабая.** Пользователи ждут бесплатный плеер; даже сильные проекты живут на донатах и Patreon (§4, п. 7). Деньги Scrobb — Pro и вовлечённость в сервис, а не плеер.
- **Отвлекает от ядра.** На этапе 1 команда делает приём скробблов, веб, расширение и десктоп-клиент. Плеер — второй продукт с собственной поддержкой: жалобы на звуковые устройства, форматы и кодеки съедают время непропорционально.
- **Десктоп-клиент уже решает задачу.** Медиасессии покрывают почти любой плеер за малую долю трудозатрат (§5.2, п. 1); точность «по медиасессиям» для скробблинга достаточна (пороги Last.fm/ListenBrainz, [architecture.md](architecture.md) §18.7).
- **Лицензии и кодеки.** FFmpeg под LGPL/GPL в зависимости от сборки, патентные вопросы по части кодеков, магазины приложений — отдельная юридическая работа **(не анализировалось)**.

### 6.4 Вывод

**Не писать свой плеер на этапах 1 и 2.** Вместо этого — шаги из §5.2: матрица плееров для десктоп-клиента, страницы «Как подключить плеер», PR в открытые плееры, импорт статистики из баз плееров, нормализация кириллицы на сервере.

**Пересмотреть решение**, если после запуска десктоп-клиента сработает хотя бы один сигнал (пороги — предложение, уточнить по данным):
- доля прослушиваний из локальных плееров среди скробблов десктоп-клиента устойчиво выше ~30% (видно по `SourceAppUserModelId` в SMTC и имени сервиса в MPRIS, [architecture.md](architecture.md) §18.6);
- повторяющиеся запросы пользователей на «свой плеер» или на функции, которые невозможны через медиасессии (точная история пропусков, синхронизация рейтингов);
- macOS: обход Now Playing перестал работать, а доля пользователей macOS заметна.

**Если всё же делать** — не «ещё один foobar2000», а **«плеер с памятью»**: минимальное, но качественное воспроизведение (gapless, ReplayGain, медиасессии) плюс то, чего нет у других: полная история, статистика и итоги, «забытые любимые», синхронизация рейтингов и плейлистов через аккаунт Scrobb. Технически — форк Museeks (MIT) на Tauri 2 с общим ядром `scrobbler-core`. Декодирование — libmpv или FFmpeg ради широты форматов; Symphonia — только если устраивают её ограничения gapless. Начать с Windows и Linux, как и десктоп-клиент.

## 7. Источники

- foobar2000: [сайт](https://www.foobar2000.org/), [changelog macOS](https://www.foobar2000.org/changelog-mac), [foo_enhanced_playcount](https://foobar2000.org/components/view/foo_enhanced_playcount), [Hydrogenaudio о версии для macOS](https://hydrogenaudio.org/index.php/topic,122517.0.html), [Rosettacheck](https://rosettacheck.com/apps/com.foobar2000.mac)
- AIMP: [блог, 2026](https://aimp.ru/blogs/?m=2026), [итоги 2025](https://www.aimp.ru/blogs/?p=1553), [анонс AIMP6](https://aimp.ru/blogs/?p=1523), [бета AIMP6](https://www.aimp.ru/?newsid=258)
- MusicBee: [VideoHelp](https://www.videohelp.com/software/MusicBee), [AlternativeTo](https://alternativeto.net/software/musicbee/?p=2)
- Strawberry: [сайт](https://www.strawberrymusicplayer.org/), [релизы](https://github.com/strawberrymusicplayer/strawberry/releases)
- fooyin: [GitHub](https://github.com/fooyin/fooyin), [Neowin](https://www.neowin.net/amp/fooyin-0126/), [LinuxToday](https://www.linuxtoday.com/blog/fooyin-v0-12-0-adds-audio-conversion-and-new-visualisations/), [AlternativeTo](https://alternativeto.net/news/2026/6/fooyin-0-11-brings-remote-stream-playback-internet-radio-improved-playlists-and-more/)
- DeaDBeeF: [загрузка](https://deadbeef.sourceforge.io/download.html), [Wikipedia](https://en.wikipedia.org/wiki/DeaDBeeF), [OMG! Ubuntu](https://omgubuntu.co.uk/2025/04/deadbeef-1-10-release-brings-new-features)
- Audacious: [4.6](https://www.audacious-media-player.org/news/64-audacious-4-6-released), [4.5](https://www.audacious-media-player.org/news/62-audacious-4-5-released)
- Quod Libet: [Wikipedia](https://en.wikipedia.org/wiki/Quod_Libet_(software))
- Tauon: [установка](https://tauonmusicbox.rocks/manual/installing/), [GitHub](https://github.com/Taiko2k/Tauon)
- Dopamine: [GitHub](https://github.com/digimezzo/dopamine), [Neowin](https://www.neowin.net/amp/dopamine-303/)
- Museeks: [GitHub](https://github.com/martpie/museeks), [VideoHelp](https://videohelp.com/software/Museeks/version-history), [AUR](https://aur.archlinux.org/packages/museeks)
- Harmonoid: [It's FOSS](https://itsfoss.com/harmonoid/), [GitHub](https://www.github.com/harmonoid)
- Rune: [GitHub](https://github.com/Losses/rune)
- Clementine: [Wikipedia](https://en.wikipedia.org/wiki/Clementine_(software)); Nuclear: [GitHub](https://github.com/nukeop/nuclear)
- Navidrome и клиенты: [Navidrome 0.64](https://alternativeto.net/news/2026/9/navidrome-0-64-brings-jellyfin-music-api-faster-artwork-delivery-and-performance-gains/), [0.63](https://opencollective.com/navidrome/updates/navidrome-v0-63-word-by-word-synchronized-lyrics-smarter-search-and-speed-improvements), [0.60](https://alternativeto.net/news/2026/2/navidrome-0-60-adds-webassembly-based-plugin-system-new-instant-mix-feature-and-more), [Feishin](https://github.com/jeffvli/feishin)
- iOS: [VOX](https://vox.rocks/blog/ios-flac-player/)
- Symphonia: [docs.rs](https://docs.rs/symphonia/^0.5)
- Рынок и тренды: [RouteNote, июль 2026](https://routenote.com/blog/music-industry-news-july-2026), [RouteNote, январь 2026](https://routenote.com/blog/music-industry-news-january-2026/)

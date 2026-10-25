# Плейлисты: стандарты, форматы, хранение, перенос в другие сервисы

> **Статус:** решение и первая часть реализованы 25.10.2026.
> - **Сделано:**
>   - в ядре: [`player/core`](../player/core/README.md), крейт `player-playlists`, `SqlitePlaylists` в `user.db`, поиск трека в библиотеке;
>   - в десктопе: [`player/shells/desktop`](../player/shells/desktop/README.md).
> - **Спроектировано, но не сделано:** синхронизация между устройствами (§7), плейлисты на сервере Scrobb и прямая отправка в сервисы (§8), умные плейлисты.
>
> Опирается на:
> - [architecture.md](architecture.md) §2.3, где описаны `tracks` и `external_ids`, и §5.3 — сопоставление и `match_key`;
> - [player-history.md](player-history.md) §1, где плейлисты отнесены к правкам пользователя;
> - [player-architecture.md](player-architecture.md) — модули `library` и `sync`.

## 0. Коротко

1. **Единого стандарта нет, есть три слоя.**
   - Локальные плееры понимают **M3U/M3U8**: путь к файлу и строка «Артист - Название».
   - Открытый структурный формат — **XSPF** и его JSON-форма **JSPF**. JSPF с расширениями MusicBrainz — родной формат плейлистов ListenBrainz.
   - Стриминги файлов не дают, только API. Переносчики (TuneMyMusic, Soundiiz) и Яндекс Музыка принимают **CSV** и **текстовый список**.
2. **Канонический формат Scrobb — JSPF**: ядро XSPF, расширение MusicBrainz и своё `https://scrobb.ru/jspf#track`. Через ListenBrainz или будущий сервер Scrobb такой плейлист уходит в Spotify и Apple Music.
3. **Остальные форматы — для совместимости:**
   - для плееров — **M3U8** и **XSPF**;
   - для переносчиков и Яндекс Музыки — **CSV** (колонки как у Exportify) и **TXT** («Артист - Название»);
   - **PLS** — только импорт.
4. **Трек в плейлисте — не ссылка на файл библиотеки.**
   - Хранится снимок метаданных, идентификаторы (MBID, ISRC, `ext_ids`) и путь на этом устройстве.
   - Поэтому плейлист переносим, а трек, которого нет в библиотеке, остаётся в плейлисте бледным и сам находится, когда файл появится.
5. **ISRC — главный ключ между стримингами.** Плеер читает его из тегов и кладёт в `ext_ids["isrc"]`: и в плейлисты, и в скробблы.
6. **Хранение — отдельная база `user.db` (SQLite).** Это правки пользователя, их нельзя пересобрать сканированием, в отличие от библиотеки.
   - **Порядок** — дробные ключи: вставка и перестановка не трогают соседей.
   - **Удаление** — надгробие.
   - **id** — UUIDv7.

   Такая схема уже готова к синхронизации.

## 1. Что принято у других

### 1.1 Локальные плееры и медиасерверы — M3U/M3U8

- **Кто:** foobar2000, AIMP, MusicBee, Winamp, VLC, Strawberry, Poweramp, Plex, Jellyfin, Navidrome.
  - Navidrome импортирует и экспортирует плейлисты командой `navidrome pls`.
  - Файлы `.m3u` из папок с музыкой Navidrome подхватывает при сканировании сам ([pls](https://www.navidrome.org/docs/usage/admin/cli/pls/)).
- **Формат** ([M3U](https://en.wikipedia.org/wiki/M3U)) — строка на запись: путь, относительный от папки плейлиста, абсолютный или URL. Расширенные директивы:

| Директива | Смысл | Как у нас |
|---|---|---|
| `#EXTM3U` | Заголовок, первая строка | Пишем; при чтении не обязателен |
| `#EXTINF:<сек>,<подпись>` | Длительность и «Артист - Название» следующей записи | Читаем и пишем. Хвост IPTV вроде `tvg-id="…"` пропускаем |
| `#PLAYLIST:` | Название плейлиста | Читаем и пишем |
| `#EXTALB:`, `#EXTART:` | Альбом и **артист альбома**, обычно один раз на список-альбом | Читаем, действуют на все записи после них. Артист альбома становится артистом трека, только если другого нет |

- **Кодировка.** Для `.m3u8` UTF-8 обязателен. Для `.m3u` берётся системная кодовая страница, поэтому старые AIMP и Winamp на русской Windows пишут **CP1251**.
  - Мы распознаём BOM UTF-8 и UTF-16.
  - Если файл не в UTF-8, читаем его как CP1251. Таблица своя, без `encoding_rs`: 64 символа для 0x80–0xBF, а «А»–«я» в 0xC0–0xFF идут подряд.
- **Ограничения.** Плейлист привязан к путям одного компьютера. Метаданных нет, кроме подписи. Poweramp `#EXTINF` вообще не пишет.

### 1.2 Открытый структурный — XSPF и JSPF

- **[XSPF](https://xspf.org/spec)** — XML-формат Xiph.
  - У трека есть `location` (URI), `identifier`, `title`, `creator`, `album`, `trackNum`, `duration` и расширения `<extension application="…">`.
  - Понимают VLC, Strawberry, Rhythmbox, Audacious.
- **[JSPF](https://musicbrainz.org/doc/jspf)** — тот же XSPF в JSON. MusicBrainz добавляет к нему расширения:
  - **плейлист** `https://musicbrainz.org/doc/jspf#playlist`: `creator`, `created_for`, `collaborators`, `copied_from`, `public`, `last_modified_at`, `additional_metadata`;
  - **трек** `https://musicbrainz.org/doc/jspf#track`: `added_by`, `added_at`, `artist_identifiers`, `release_identifier`, `additional_metadata`;
  - **запись MusicBrainz** — `identifier`: `https://musicbrainz.org/recording/<MBID>`.
- **ListenBrainz** хранит плейлисты в JSPF и отдаёт через [API](https://listenbrainz.readthedocs.io/en/latest/users/api/playlist.html). Эндпоинты:
  - `POST /1/playlist/create`;
  - `GET /1/playlist/(mbid)` и `/xspf`;
  - `item/add`, `item/move`, `item/delete`;
  - **экспорт в сервис:** `POST /1/playlist/(mbid)/export/(service)` и `POST /1/playlist/export-jspf/(service)` — второй экспортирует JSPF из тела запроса, без сохранения в ListenBrainz;
  - **импорт:** `GET /1/playlist/{spotify,apple_music,soundcloud}/(id)/tracks`.

  Это готовый открытый «мост» между стримингами, и он говорит на JSPF.

### 1.3 Стриминги и переносчики

| Сервис | Что есть | Что это значит для нас |
|---|---|---|
| **Spotify** | Только Web API. С 11.02.2026 Development Mode ([гайд](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide)): у владельца приложения должен быть Premium, не больше 5 пользователей, один Client ID (с июля 2026 — до 25). Работа с треками плейлиста переехала с `/tracks` на `/items`, содержимое отдаётся только по своим и совместным плейлистам. Extended Quota ([quota modes](https://developer.spotify.com/documentation/web-api/concepts/quota-modes)) с 15.05.2025 дают только организациям примерно от 250 тыс. MAU | Прямую интеграцию для всех пользователей Scrobb на старте не получить. Путь — экспорт через ListenBrainz (§1.2) или через переносчики (CSV) |
| **Яндекс Музыка** | Публичного API и экспорта нет. Есть страница импорта `music.yandex.ru/import`: текстовый список или файл `.txt`, `.pls`, `.m3u` ([Хабр](https://habr.com/ru/post/655869)). Статья 2022 года, интерфейс мог измениться | Экспорт в **TXT** «Артист - Название» и вставка на страницу импорта. Или переносчик, например [TuneMyMusic: текстовый файл → Яндекс Музыка](https://www.tunemymusic.com/ru/transfer/text-file-to-yandex-music) |
| **Apple Music** | MusicKit API — нужен аккаунт Apple Developer Program | Через ListenBrainz или переносчики |
| **TuneMyMusic** | Принимает CSV и текст, ищет по названию, артисту, альбому и **ISRC** ([CSV → Spotify](https://www.tunemymusic.com/it/transfer/csv-to-spotify)) | CSV с колонкой ISRC |
| **Soundiiz** | «Import Playlist → From File»: CSV, XSPF, M3U ([XSPF → Soundiiz](https://soundiiz.com/tutorial/import-xspf-to-soundiiz)) | CSV или XSPF |
| **Exportify** | Выгрузка плейлистов Spotify в CSV: `Track URI, ISRC, Track Name, Album Name, Artist Name(s), Release Date, Duration (ms), …` ([exportify](https://github.com/pavelkomarov/exportify)) | Читаем его CSV, свои колонки называем так же |

### 1.4 Выводы

- **Структурный формат — JSPF.** Он хранит всё, что нам нужно, совместим с ListenBrainz и расширяется без поломки чужих читателей.
- **Трек опознаётся по идентификаторам и метаданным, а не по пути.** Порядок:
  1. MBID — в мире MusicBrainz и ListenBrainz;
  2. ISRC — в стримингах;
  3. «артист + название» с нормализацией — везде.

  Путь работает только на своём компьютере.
- **M3U8, CSV и TXT нужны, потому что их принимают другие.** Источником истины они быть не могут: теряют метаданные.

## 2. Форматы в Scrobb

| Формат | Импорт | Экспорт | Для кого | Что сохраняется |
|---|---|---|---|---|
| **M3U / M3U8** | ✓ | ✓ (`.m3u8`, UTF-8) | Любой плеер, Navidrome, Poweramp | Путь, длительность, «Артист - Название». Треки без файла на этом компьютере не попадают в файл, о чём говорит уведомление |
| **XSPF** | ✓ | ✓ | VLC, Strawberry, Rhythmbox, Soundiiz | Плюс `file://`-URI с процентным кодированием, альбом, MBID в `identifier` |
| **JSPF** | ✓ | ✓ | ListenBrainz, Scrobb, перенос между нашими устройствами | Всё: артисты списком, артист альбома, `ext_ids` (ISRC, Spotify и др.), время добавления, id плейлиста |
| **CSV** | ✓ | ✓ | TuneMyMusic, Soundiiz, Excel, Exportify | Название, артисты, альбом, артист альбома, длительность, ISRC, MBID, путь. Импорт узнаёт колонки по названиям, в том числе русским; разделитель `,`, `;` (Excel) или таб; кавычки по RFC 4180 |
| **TXT** | ✓ | ✓ | Импорт Яндекс Музыки, переносчики | Только «Артист - Название». При импорте разделители ` - `, ` — `, ` – `, нумерация «1.» отрезается |
| **PLS** | ✓ | — | Старые плееры, радио | Путь, название, длительность |

**Формат файла определяется по расширению.** CSV — по `.csv` и `.tsv`. Если расширение незнакомое, формат определяется по началу файла:
- `#EXTM3U` — M3U;
- `[playlist]` — PLS;
- `<?xml` или `<playlist` — XSPF;
- `{` — JSPF;
- иначе — текстовый список.

**Пример JSPF Scrobb** (сокращён):

```json
{ "playlist": {
    "title": "Вечер на кухне",
    "annotation": "Тихое на вечер",
    "date": "2026-10-25T09:00:00Z",
    "extension": {
      "https://musicbrainz.org/doc/jspf#playlist": { "public": false, "last_modified_at": "2026-10-25T09:30:00Z" },
      "https://scrobb.ru/jspf#playlist": { "id": "01a17274-6494-7616-a03b-7020ff4144ee" } },
    "track": [ {
      "title": "Кукушка", "creator": "Кино", "album": "Чёрный альбом", "duration": 397000,
      "identifier": ["https://musicbrainz.org/recording/<MBID>"],
      "location": ["file:///home/ivan/%D0%9C%D1%83%D0%B7%D1%8B%D0%BA%D0%B0/Kino/kukushka.flac"],
      "extension": {
        "https://musicbrainz.org/doc/jspf#track": { "added_at": "2026-10-25T09:05:00Z" },
        "https://scrobb.ru/jspf#track": { "artists": ["Кино"], "album_artist": "Кино", "ext_ids": { "isrc": "RUA018700012" } } } } ] } }
```

Чужой читатель увидит обычный JSPF, ListenBrainz — свои поля, а Scrobb восстановит плейлист без потерь. При чтении строки и массивы в `identifier`, `location` и `creator` принимаются в любой форме: разные программы пишут по-разному.

## 3. Модель данных

```
Playlist     { id: UUIDv7, title, description?, origin?, created_at_ms, updated_at_ms }
PlaylistItem { id: UUIDv7, position: ключ порядка, added_at_ms,
               location?: путь на этом устройстве,
               track: TrackSnapshot { title, artists[], album?, album_artist?, duration_ms?,
                                      recording_mbid?, ext_ids{isrc, spotify, …} } }
```

- **`TrackSnapshot`** — тот же снимок, что в истории прослушиваний ([player-history.md](player-history.md) §3), теперь он в `player-domain`.
  - Снимок не обновляется вслед за тегами: плейлист помнит, что в него положили.
  - Что сейчас играет, решает поиск по библиотеке (§4).
- **`origin`** показывает, откуда плейлист: `null` — создан в плеере, `import:m3u`, `import:jspf` и т. д.
- **Порядок.** `position` — дробный ключ в base-62 (`0-9A-Za-z`, никогда не оканчивается на `0`).
  - `between(a, b)` даёт ключ строго между соседями, `spread(a, b, n)` — n ключей для вставки пачки.
  - Перестановка меняет одну строку, поэтому правки с двух устройств сливаются без перенумерации.
  - При равных ключах (одновременная вставка на двух устройствах) порядок решает `id`. На этот случай индекс `(playlist_id, position, id)`.
- **Удаление** — надгробие `deleted_at_ms`: синхронизации нужно знать, что элемент удалён, а не «ещё не пришёл».
- **Хранилище** — трейт `PlaylistStore`. Детерминированный: id, время и позиции приходят от вызывающего, поэтому тесты воспроизводимы.
  - `MemoryPlaylists` — для тестов и как образец.
  - `SqlitePlaylists` — `user.db`, таблицы `STRICT`, WAL, миграции по `user_version`. Как в `history.db`, базу более новой версии ядро не открывает.

  Обе реализации проходят общий набор `player-testkit::playlists::conformance`.

```sql
CREATE TABLE playlists (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, origin TEXT,
  created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL, deleted_at_ms INTEGER) STRICT;
CREATE TABLE playlist_items (id TEXT PRIMARY KEY, playlist_id TEXT NOT NULL REFERENCES playlists (id),
  position TEXT NOT NULL, added_at_ms INTEGER NOT NULL, location TEXT,
  title TEXT NOT NULL, artists TEXT NOT NULL /* JSON */, album TEXT, album_artist TEXT,
  duration_ms INTEGER, recording_mbid TEXT, ext_ids TEXT NOT NULL DEFAULT '{}', deleted_at_ms INTEGER) STRICT;
CREATE INDEX playlist_items_order ON playlist_items (playlist_id, position, id) WHERE deleted_at_ms IS NULL;
```

**Почему отдельный `user.db`.** Библиотека — кэш файлов: её можно удалить и пересканировать. История и правки пользователя невосполнимы. Позже в `user.db` лягут рейтинги, «любимые» и журнал синхронизации.

## 4. Как трек плейлиста находится в библиотеке

`player-library::LibraryIndex` ведётся при каждом добавлении и удалении трека. Запрос — `TrackQuery { location, track }`, порядок поиска:

1. **Путь**: сначала точное совпадение, затем без учёта регистра и вида разделителей `\` и `/` — плейлисты с Windows.
2. **MBID записи.**
3. **ISRC.** Читается из тегов (`ItemKey::Isrc`) и хранится без дефисов, в верхнем регистре.
4. **Ключ «артист + название»** — `scrobb-sync::TrackKey`, нормализация как у сервера:
   - NFKC;
   - «ё» → «е»;
   - пробелы схлопнуты;
   - casefold;
   - из названия убрано «(feat. …)», «ft.», «при участии»;
   - артист — главный, до `feat.`/`ft.`/`featuring`/`при уч.`/`при участии`. Знак `&` и запятая артиста не делят: «Simon & Garfunkel» — один артист.

   Векторы — в [`contracts/match-key/v1.json`](../contracts/match-key/v1.json), их проходит ядро плеера, позже бэкенд.

Если кандидатов несколько, выигрывает тот же альбом, затем ближайшая длительность. Если обе длительности известны и расходятся больше чем на 10 с, это другая запись: живая версия или ремикс.

Поиск идёт при каждом открытии плейлиста, а не при импорте. Поэтому трек, которого не было, находится сам, как только папка просканирована. Плеер проверен на M3U8 c путём `D:\Музыка\…mp3` и подписью «КИНО feat. Кто-то - Спокойная Ночь»: трек нашёлся по ключу.

## 5. Импорт и экспорт

- **Импорт** — `Core::import_playlist(file_name, bytes, base_dir)`:
  - определяет формат и кодировку;
  - превращает относительные пути в абсолютные от папки файла (`..`, `\`, `file://`-URI с процентами);
  - дополняет снимок данными из библиотеки, если трек нашёлся (пустые поля — из тегов, заполненные остаются из файла);
  - создаёт плейлист и отдаёт отчёт `{ playlist, total, matched }`.

  В десктопе импорт вызывают из меню и из «Открыть с помощью» для `.m3u`, `.m3u8`, `.pls`, `.xspf`, `.jspf` — тогда окно сразу открывает плейлист. CSV и TXT импортируются только через меню: по расширению их не отличить от таблиц и заметок.
- **Экспорт** — `Core::export_playlist(id, format, target_dir)`:
  - берёт путь из библиотеки, если трек нашёлся;
  - пишет пути в M3U8 относительно папки файла, если у них общая папка, иначе абсолютными;
  - отдаёт текст и число треков, которые не попали в M3U8 без пути. Файл пишет оболочка.

## 6. Интерфейс десктопа

- **Слева** — «Плейлисты | Папки». В списке плейлистов — мозаика обложек до 4 альбомов, кнопки «Импорт» и «Создать».
- **Экран плейлиста:**
  - шапка: название, описание, «N треков · длительность · K нет в библиотеке»;
  - «Играть» — в очередь попадают только найденные треки;
  - «Экспорт» с подсказкой, какой формат для чего, и «⋯» с пунктами «Название и описание» и «Удалить».
- **Добавить в плейлист** — из меню трека, шапки альбома и артиста, панели «Сейчас играет» или перетаскиванием трека на плейлист слева.
- **Порядок** меняется перетаскиванием строк, а с клавиатуры — пунктами меню «Выше» и «Ниже».
- **Поиск ⌘K** находит и плейлисты.

## 7. Синхронизация между устройствами (спроектировано)

Схема готова, миграция не понадобится:

- **Плейлист.** Поля `title` и `description` — «последняя запись побеждает» по каждому полю на гибридных логических часах (`scrobb-sync::{Hlc, Lww}`). Удаление — надгробие, оно побеждает правки с меньшими часами.
- **Элементы — множество по `id`.**
  - Добавление — новый id.
  - Удаление — надгробие.
  - Перестановка — LWW на `position`. Одновременные перестановки одного трека: побеждает последняя. Разных треков: обе применяются, ключи независимы.
- **Журнал.** Таблицы `sync_ops` и `sync_state` в `user.db`, как в [player-history.md](player-history.md) §6, п. 4.
- **Пути не синхронизируются**: `location` — свой на каждом устройстве. На другом устройстве трек ищется по MBID, ISRC или ключу.

## 8. Предложение для сервера Scrobb

[architecture.md](architecture.md) §9.6 говорил «плейлисты не делаем». Плеер это меняет: плейлисты нужны для синхронизации и переноса. Предложение на этап 2:

1. **Таблицы:**
   - **`playlists`**: `id uuid`, `user_id`, `title`, `description`, `public`, `origin`, `created_at`, `updated_at`, `deleted_at`, `hlc`;
   - **`playlist_items`**:
     - `id uuid`, `playlist_id`, `position text` — тот же дробный ключ;
     - `track_id` — сопоставлен через `external_ids`, `match_key` и `track_aliases`, как скробблы (§5.3);
     - `snapshot jsonb` — то, что прислал клиент, на случай если сопоставление ошибётся;
     - `added_at`, `deleted_at`, `hlc`.
2. **API в JSPF, совместимый с ListenBrainz `/1/playlist/*`:**
   - `create`;
   - `GET (id)`;
   - `edit`;
   - `item/add`, `item/move`, `item/delete`;
   - `delete`.

   Плееры Scrobb используют его же для синхронизации, плюс курсор изменений `GET /api/v1/sync/playlists?since=`. Сторонние клиенты, которые умеют ListenBrainz, получают плейлисты бесплатно — так уже работает приём скробблов (§3.2).
3. **Экспорт в сервисы — с сервера, а не из каждого клиента:**
   - **Spotify, Apple Music** — пока у Scrobb нет Extended Quota (§1.3), через `POST /1/playlist/export-jspf/(service)` ListenBrainz от имени пользователя, который привязал там свой Spotify. Позже — свой OAuth.
   - **Яндекс Музыка** — TXT для страницы импорта; прямой путь появится вместе с её API.
   - **Остальные** — CSV для переносчиков.
4. **Веб-приложение** показывает и правит те же плейлисты, профиль получает вкладку «Плейлисты».

## 9. Что дальше

- **Умные плейлисты** — правила поверх библиотеки и истории. Отложены по решению Ивана, модель их не задевает: это другой `origin` и запрос вместо элементов.
- **Прямые интеграции** — после сервера (§8).
- **Обложка плейлиста** — своя картинка вместо мозаики.
- **«Найти недостающие»** — подсказать, в какой папке может быть трек, или открыть его в стриминге по ISRC.

## Источники

- M3U и расширенные директивы: <https://en.wikipedia.org/wiki/M3U>
- XSPF v1: <https://xspf.org/spec>
- JSPF и расширения MusicBrainz: <https://musicbrainz.org/doc/jspf>
- ListenBrainz, API плейлистов: <https://listenbrainz.readthedocs.io/en/latest/users/api/playlist.html>
- Spotify, изменения Development Mode 02.2026: <https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide>, <https://developer.spotify.com/blog/2026-02-06-update-on-developer-access-and-platform-security>
- Spotify, режимы квот: <https://developer.spotify.com/documentation/web-api/concepts/quota-modes>
- Navidrome `pls`: <https://www.navidrome.org/docs/usage/admin/cli/pls/>
- Exportify: <https://github.com/pavelkomarov/exportify>
- TuneMyMusic: <https://www.tunemymusic.com/it/transfer/csv-to-spotify>, <https://www.tunemymusic.com/ru/transfer/text-file-to-yandex-music>
- Soundiiz: <https://soundiiz.com/tutorial/import-xspf-to-soundiiz>
- Импорт в Яндекс Музыку: <https://habr.com/ru/post/655869>

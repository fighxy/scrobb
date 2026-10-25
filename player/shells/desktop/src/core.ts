// Типы ядра в JSON — повторяют `player/core/crates/player-core/tests/json.rs`.
// Перечисления ядра приходят как `{ type, data }`.

export type TrackId = number;

export interface ReplayGain {
  track_gain_db: number | null;
  track_peak: number | null;
  album_gain_db: number | null;
  album_peak: number | null;
}

export interface Track {
  id: TrackId;
  uri: string;
  title: string;
  artists: string[];
  album: string | null;
  album_artist: string | null;
  duration_ms: number | null;
  track_no: number | null;
  disc_no: number | null;
  recording_mbid: string | null;
  isrc: string | null;
  replay_gain: ReplayGain;
}

export type PlaybackStatus = "stopped" | "buffering" | "playing" | "paused";
export type RepeatMode = "off" | "all" | "one";

export interface PlayerState {
  status: PlaybackStatus;
  current: Track | null;
  position_ms: number;
  queue_position: [number, number] | null;
  repeat: RepeatMode;
  shuffle: boolean;
  volume: number;
}

export type ScrobbleState =
  | "not_linked"
  | "linking"
  | "ready"
  | "retrying"
  | "unauthorized"
  | "upgrade_required";

export interface ScrobbleStatus {
  pending: number;
  state: ScrobbleState;
}

export type Command =
  | { type: "play_queue"; data: { tracks: TrackId[]; start: number } }
  | { type: "pause" }
  | { type: "resume" }
  | { type: "stop" }
  | { type: "next" }
  | { type: "previous" }
  | { type: "seek"; data: { position_ms: number } }
  | { type: "set_repeat"; data: RepeatMode }
  | { type: "set_volume"; data: number }
  | { type: "link_scrobb_account"; data: { device_name: string } }
  | { type: "cancel_scrobb_link" }
  | { type: "unlink_scrobb_account" };

export type CoreEvent =
  | { type: "state"; data: PlayerState }
  | { type: "scrobb_link_code"; data: { user_code: string; verification_uri_complete: string } }
  | { type: "scrobbling"; data: ScrobbleStatus }
  | { type: "error"; data: string };

export type CoreError =
  | { type: "not_implemented" | "missing_port" | "not_configured" | "history" | "playlist"; data: string }
  | { type: "invalid_queue" };

export type Outcome = "playing" | "finished" | "skipped" | "stopped" | "error" | "interrupted";

/** Метаданные трека без привязки к файлу (`player_domain::TrackSnapshot`). */
export interface TrackSnapshot {
  title: string;
  artists: string[];
  album: string | null;
  album_artist: string | null;
  duration_ms: number | null;
  recording_mbid: string | null;
  /** `isrc`, `spotify`… — ключи как у `ext_ids` сервера. */
  ext_ids: Record<string, string>;
}

export interface PlayRecord {
  id: string;
  started_at_ms: number;
  ended_at_ms: number | null;
  played_ms: number;
  outcome: Outcome;
  scrobbled: boolean;
  track: {
    title: string;
    artists: string[];
    album: string | null;
    album_artist: string | null;
    duration_ms: number | null;
    recording_mbid: string | null;
    ext_ids: Record<string, string>;
  };
  track_id: TrackId | null;
  device_id: string;
  origin: string;
}

/** Ход сканирования папки (`library::Scanning`). */
export interface Scanning {
  folder: string;
  /** Сколько аудиофайлов найдено. */
  found: number;
  /** У скольких прочитаны теги. */
  read: number;
}

/** События библиотеки оболочки (`library://event`). */
export type LibraryEvent =
  | { type: "scanning"; data: Scanning }
  | { type: "changed" }
  | { type: "playlists" }
  | { type: "error"; data: string };

/** Всё, что окну нужно при открытии (`bridge::Snapshot`). */
export interface Snapshot {
  state: PlayerState;
  tracks: Track[];
  /** Папки-источники: запоминаются и сканируются при запуске. */
  folders: string[];
  scanning: Scanning | null;
  scrobbling: ScrobbleStatus | null;
  scrobbling_available: boolean;
  /** Плейлист, с которым запустили приложение («Открыть с помощью»). */
  opened_playlist: PlaylistId | null;
}

export interface Added {
  tracks: Track[];
  failed: string[];
}

export type PlaylistId = string;
export type ItemId = string;

export interface Playlist {
  id: PlaylistId;
  title: string;
  description: string | null;
  /** `null` — создан здесь; `import:m3u` и т. п. — импорт. */
  origin: string | null;
  created_at_ms: number;
  updated_at_ms: number;
}

/** Плейлист в списке (`player_core::PlaylistCard`). */
export interface PlaylistCard {
  playlist: Playlist;
  items: number;
  duration_ms: number;
  /** Треки библиотеки для мозаики обложки. */
  covers: TrackId[];
}

/** Элемент плейлиста (`player_core::ResolvedItem`). */
export interface PlaylistEntry {
  id: ItemId;
  position: string;
  added_at_ms: number;
  location: string | null;
  track: TrackSnapshot;
  /** Трек библиотеки; `null` — на этом устройстве такого нет. */
  track_id: TrackId | null;
}

export interface PlaylistView {
  playlist: Playlist;
  items: PlaylistEntry[];
}

export interface ImportReport {
  playlist: Playlist;
  total: number;
  matched: number;
}

export type PlaylistFormat = "m3u" | "xspf" | "jspf" | "csv" | "txt";

export interface Exported {
  text: string;
  /** Сколько не попало (в M3U8 — треки без файла на этом устройстве). */
  skipped: number;
}

/** Ядро для окна: в приложении — команды Tauri, в браузере — подмена (`mock.ts`). */
export interface Backend {
  dispatch(command: Command): Promise<void>;
  snapshot(): Promise<Snapshot>;
  pickFiles(): Promise<string[]>;
  addFiles(paths: string[]): Promise<Added>;
  pickFolder(): Promise<string | null>;
  /** `false` — папка уже входит в одну из добавленных. */
  addFolder(path: string): Promise<boolean>;
  removeFolder(path: string): Promise<void>;
  rescan(): Promise<void>;
  onLibraryEvent(handler: (event: LibraryEvent) => void): Promise<() => void>;
  recentPlays(beforeMs: number | null, limit: number): Promise<PlayRecord[]>;
  onEvent(handler: (event: CoreEvent) => void): Promise<() => void>;
  /** Открыть ссылку системным браузером. */
  openUrl(url: string): Promise<void>;
  /** Треки очереди по порядку — для «Далее». */
  queue(): Promise<TrackId[]>;
  /** Адрес обложки для `<img>`; если обложки нет, картинка не загрузится. */
  coverUrl(id: TrackId, size: CoverSize): string;
  /** Цвет обложки для фона под белым текстом (`#rrggbb`) или `null`. */
  coverColor(id: TrackId): Promise<string | null>;
  /** Показать файл в файловом менеджере. */
  revealInFolder(path: string): Promise<void>;
  playlists(): Promise<PlaylistCard[]>;
  playlist(id: PlaylistId): Promise<PlaylistView>;
  /** Новый плейлист, сразу с треками. */
  createPlaylist(title: string, tracks: TrackId[]): Promise<Playlist>;
  updatePlaylist(id: PlaylistId, title: string, description: string | null): Promise<void>;
  deletePlaylist(id: PlaylistId): Promise<void>;
  /** Ответ — сколько добавлено. `at` — место, `null` — в конец. */
  addToPlaylist(id: PlaylistId, tracks: TrackId[], at: number | null): Promise<number>;
  removeFromPlaylist(id: PlaylistId, items: ItemId[]): Promise<void>;
  moveInPlaylist(id: PlaylistId, item: ItemId, to: number): Promise<void>;
  /** Диалог выбора файла плейлиста; `null` — отказались. */
  pickPlaylistFile(): Promise<string | null>;
  importPlaylist(path: string): Promise<ImportReport>;
  /** Диалог сохранения; `null` — отказались. */
  pickExportPath(defaultName: string, format: PlaylistFormat): Promise<string | null>;
  exportPlaylist(id: PlaylistId, format: PlaylistFormat, path: string): Promise<Exported>;
}

/** Форматы экспорта: расширение и подсказка, куда его нести. */
export const EXPORT_FORMATS: Array<{ format: PlaylistFormat; ext: string; label: string; hint: string }> = [
  { format: "m3u", ext: "m3u8", label: "M3U8", hint: "Любой плеер: foobar2000, AIMP, VLC, Poweramp, Navidrome" },
  { format: "xspf", ext: "xspf", label: "XSPF", hint: "VLC, Strawberry, Rhythmbox — с названиями и альбомами" },
  { format: "jspf", ext: "jspf", label: "JSPF", hint: "ListenBrainz и Scrobb: MusicBrainz ID, ISRC, все артисты" },
  { format: "csv", ext: "csv", label: "CSV", hint: "TuneMyMusic, Soundiiz → Spotify, Яндекс, Apple Music; таблицы" },
  { format: "txt", ext: "txt", label: "Текст", hint: "«Артист - Название»: импорт Яндекс Музыки, переносчики" },
];

/** `s` — 128 px для строк и плиток, `l` — 640 px для «Сейчас играет» и шапок. */
export type CoverSize = "s" | "l";

/** Текст ошибки команды для пользователя. */
export function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "type" in error) {
    const e = error as CoreError;
    switch (e.type) {
      case "invalid_queue":
        return "Пустая очередь";
      case "missing_port":
        return `Нет адаптера: ${e.data}`;
      case "not_configured":
        return `Не настроено: ${e.data}`;
      case "history":
        return `История: ${e.data}`;
      case "playlist":
        return e.data;
      case "not_implemented":
        return `Ещё не сделано: ${e.data}`;
    }
  }
  return String(error);
}

/** Имя устройства для «Приложений» на сайте Scrobb. */
export function deviceName(userAgent = navigator.userAgent): string {
  const os = /Windows/.test(userAgent)
    ? "Windows"
    : /Mac OS/.test(userAgent)
      ? "macOS"
      : /Linux/.test(userAgent)
        ? "Linux"
        : "компьютер";
  return `Scrobb Player · ${os}`;
}

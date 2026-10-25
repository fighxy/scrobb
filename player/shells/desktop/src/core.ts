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
  | { type: "not_implemented" | "missing_port" | "not_configured" | "history"; data: string }
  | { type: "invalid_queue" };

export type Outcome = "playing" | "finished" | "skipped" | "stopped" | "error" | "interrupted";

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
}

export interface Added {
  tracks: Track[];
  failed: string[];
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
}

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

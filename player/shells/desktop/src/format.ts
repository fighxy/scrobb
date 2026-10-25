import type { Outcome, PlayerState, PlaylistSyncStatus } from "./core";

/** `3:07`, `1:02:05`; неизвестно — `—`. */
export function duration(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/**
 * Позиция «сейчас»: ядро присылает снимок не чаще нескольких раз в секунду,
 * а между снимками окно двигает ползунок само (`docs/player-architecture.md` §3.3).
 */
export function livePosition(state: PlayerState, receivedAt: number, now: number): number {
  if (state.status !== "playing") return state.position_ms;
  const moved = state.position_ms + Math.max(0, now - receivedAt);
  return Math.min(moved, state.current?.duration_ms ?? Number.POSITIVE_INFINITY);
}

/** Время прослушивания в истории: сегодня — `14:05`, иначе — `3 окт., 14:05`. */
export function playedAt(ms: number, now = Date.now()): string {
  const date = new Date(ms);
  const time = date.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  return new Date(now).toDateString() === date.toDateString()
    ? time
    : `${date.toLocaleDateString("ru-RU", { day: "numeric", month: "short" })}, ${time}`;
}

/** Когда это было: «только что», «5 мин назад», «в 14:05», «3 окт., 14:05». */
export function ago(ms: number, now = Date.now()): string {
  const passed = now - ms;
  if (passed < 60_000) return "только что";
  if (passed < 3_600_000) return `${Math.floor(passed / 60_000)}\u00a0мин назад`;
  const at = playedAt(ms, now);
  return at.includes(",") ? at : `в ${at}`;
}

/** Подпись состояния синхронизации плейлистов для подсказки значка. */
export function syncLabel(status: PlaylistSyncStatus | null, now = Date.now()): string {
  const last = status?.last_synced_ms ?? null;
  switch (status?.state ?? "not_linked") {
    case "not_linked":
      return "Подключите Scrobb — плейлисты будут на всех ваших устройствах";
    case "syncing":
      return "Синхронизация плейлистов…";
    case "synced":
      return last ? `Плейлисты синхронизированы ${ago(last, now)}` : "Плейлисты синхронизированы";
    case "retrying":
      return `Нет связи со Scrobb — правки уйдут позже${last ? ` (последний раз ${ago(last, now)})` : ""}`;
    case "unauthorized":
      return "Войдите в Scrobb заново — синхронизация плейлистов стоит";
    case "upgrade_required":
      return "Обновите плеер — синхронизация плейлистов стоит";
    case "unsupported":
      return "Сервер Scrobb пока не умеет синхронизировать плейлисты — они хранятся на этом устройстве";
  }
}

export const OUTCOME: Record<Outcome, string> = {
  playing: "играет",
  finished: "дослушан",
  skipped: "пропущен",
  stopped: "остановлен",
  error: "ошибка",
  interrupted: "прервано",
};

/** Имя папки для подписи: последняя часть пути. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? path;
}

export function artists(list: string[]): string {
  return list.length ? list.join(", ") : "Неизвестный артист";
}

export type Forms = readonly [one: string, few: string, many: string];

export const TRACKS: Forms = ["трек", "трека", "треков"];
export const ALBUMS: Forms = ["альбом", "альбома", "альбомов"];

/** Форма слова для числа: 1 трек, 3 трека, 5 треков, 11 треков, 21 трек. */
export function plural(n: number, [one, few, many]: Forms): string {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return one;
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return few;
  return many;
}

/** «12 345 треков»: разряды — неразрывным пробелом, по русской норме. */
export function count(n: number, forms: Forms): string {
  return `${n.toLocaleString("ru-RU")}\u00a0${plural(n, forms)}`;
}

/** Длительность альбома или списка: «47 мин», «2 ч 05 мин». */
export function longDuration(ms: number): string {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes}\u00a0мин`;
  return `${Math.floor(minutes / 60)}\u00a0ч ${String(minutes % 60).padStart(2, "0")}\u00a0мин`;
}
